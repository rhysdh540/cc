use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use base64::Engine;
use clap::Parser;
use rand::RngExt;
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition, TableHandle};
use serde::Serialize;
use anyhow::Result;
use axum::{
    body::Bytes,
    Router,
    extract::{State, Path},
    Json,
    http::{StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response as AxumResponse},
    routing::{get, post}
};
use tokio::net::TcpListener;

#[derive(Debug, Clone, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Parser)]
enum Commands {
    Serve {
        /// Path to the database file.
        #[arg()]
        db: PathBuf,

        /// Base URL for shortened links.
        #[arg(long, default_value = "127.0.0.1:8080")]
        url: SocketAddr,

        /// Path to an html file to serve on the root path.
        #[arg(long)]
        index: Option<PathBuf>,
    },
    /// List all code -> url mappings in the database.
    #[command(name = "ls")]
    List {
        /// Path to the database file.
        #[arg()]
        db: PathBuf,
    }
}

#[derive(Serialize)]
struct Response {
    ok: bool,
    msg: String // either the code or an error message
}

const CODE_TO_URL: TableDefinition<&str, &str> = TableDefinition::new("c2u");
const URL_TO_CODE: TableDefinition<&str, &str> = TableDefinition::new("u2c");
const ALLOWED_SCHEMES: &[&str] = &["http", "https"];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { db, url, index } => serve(db, url, index).await?,
        Commands::List { db } => list(db)?,
    }

    Ok(())
}

fn list(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if !path.is_file() {
        eprintln!("database file does not exist or is not a file: {}", path.display());
        std::process::exit(1);
    }
    let db = Database::open(&path)?;
    let rd = db.begin_read()?;
    let rd_c2u = rd.open_table(CODE_TO_URL)?;

    println!("{} mapping{} found in {}:",
             rd_c2u.len()?, if rd_c2u.len()? == 1 { "" } else { "s" }, path.display());
    rd_c2u.iter()?.for_each(|res| {
        if let Ok((code, url)) = res {
            println!("  {} -> {}", code.value(), url.value());
        } else {
            println!("  error reading mapping: {}", res.err().unwrap());
        }
    });

    Ok(())
}

async fn serve(
    path: PathBuf,
    url: SocketAddr,
    index: Option<PathBuf>
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let db = Arc::new(Database::create(&path)?);

    if !db.begin_read()?.list_tables()?.any(|tb| tb.name() == CODE_TO_URL.name()) {
        let wr = db.begin_write()?;
        wr.open_table(CODE_TO_URL)?;
        wr.open_table(URL_TO_CODE)?;
        wr.commit()?;
    }

    println!("Starting cc at http://{}, db at {}", url, path.display());

    let mut app = Router::new()
        .route("/put", post(put_new))
        .route("/{code}", get(get_code))
        .with_state(db);

    if let Some(index) = &index {
        if !index.is_file() {
            eprintln!("index file does not exist or is not a file: {}", index.display());
            std::process::exit(1);
        }

        let index = Html(fs::read_to_string(index)?);
        app = app.route("/", get(move || async { index }));
    }

    app = app.fallback_service(get(|| async { StatusCode::NOT_FOUND }));

    let listener = TcpListener::bind(url).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

macro_rules! nope {
    ($e:expr) => {
        {
            println!("db error: {}", $e);
            let j = Json(Response { ok: false, msg: "problem with database".to_string() });
            return (StatusCode::INTERNAL_SERVER_ERROR, j).into_response();
        }
    };
}

async fn get_code(State(db): State<Arc<Database>>, code: Path<String>) -> AxumResponse {
    let rd = match db.begin_read() {
        Ok(rd) => rd,
        Err(e) => nope!(e),
    };

    let rd_c2u = match rd.open_table(CODE_TO_URL) {
        Ok(tb) => tb,
        Err(e) => nope!(e)
    };

    return match rd_c2u.get(code.as_str()) {
        Ok(Some(url)) => {
            println!("found code {} -> {}", code.as_str(), url.value());
            Redirect::permanent(url.value()).into_response()
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => nope!(e)
    }
}

async fn put_new(State(db): State<Arc<Database>>, raw_url: Bytes) -> AxumResponse {
    let mut str_url = match std::str::from_utf8(&raw_url) {
        Ok(u) => u.trim().to_string(),
        Err(e) => {
            let j = Json(Response { ok: false, msg: format!("invalid utf-8 in url: {}", e) }).into_response();
            return (StatusCode::BAD_REQUEST, j).into_response();
        }
    };

    let url: Uri = match str_url.parse() {
        Ok(u) => u,
        Err(e) => {
            let j = Json(Response { ok: false, msg: format!("invalid url: {}", e) }).into_response();
            return (StatusCode::BAD_REQUEST, j).into_response();
        }
    };

    str_url = url.to_string(); // normalize the url

    if let Some(scheme) = url.scheme_str() {
        if !ALLOWED_SCHEMES.contains(&scheme) {
            let j = Json(Response { ok: false, msg: format!("unsupported url scheme: {}", scheme) }).into_response();
            return (StatusCode::BAD_REQUEST, j).into_response();
        }
    } else {
        let j = Json(Response { ok: false, msg: "url missing scheme".to_string() }).into_response();
        return (StatusCode::BAD_REQUEST, j).into_response();
    }

    let wr = match db.begin_write() {
        Ok(wr) => wr,
        Err(e) => nope!(e),
    };

    let mut wr_u2c = match wr.open_table(URL_TO_CODE) {
        Ok(tb) => tb,
        Err(e) => nope!(e),
    };

    let mut wr_c2u = match wr.open_table(CODE_TO_URL) {
        Ok(tb) => tb,
        Err(e) => nope!(e),
    };
    match wr_u2c.get(str_url.as_str()) {
        Ok(Some(code)) => {
            let code = code.value().to_string();
            return Json(Response { ok: true, msg: code }).into_response();
        }
        Ok(None) => {}
        Err(e) => nope!(e),
    }

    // make sure code is unique
    let mut code = gen_key();
    loop {
        // this may overwrite something in the astronomically small case that
        // another writer inserts the same code after this and before the commit
        // but its fine lol
        match wr_c2u.get(code.as_str()) {
            Ok(None) => break,
            Ok(Some(_)) => code = gen_key(),
            Err(e) => nope!(e),
        }
    }

    if let Err(e) = wr_c2u.insert(code.as_str(), str_url.as_str()) {
        nope!(e)
    }

    if let Err(e) = wr_u2c.insert(str_url.as_str(), code.as_str()) {
        nope!(e)
    }

    drop(wr_u2c);
    drop(wr_c2u);

    if let Err(e) = wr.commit() {
        nope!(e)
    }

    println!("stored: {} -> {}", code.as_str(), url);
    let j = Json(Response { ok: true, msg: code.to_string() }).into_response();
    return (StatusCode::CREATED, j).into_response();
}

fn gen_key() -> String {
    let mut bytes = [0u8; 4];
    rand::rng().fill(&mut bytes);
    return base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&bytes);
}