# cc

makes urls short

## usage

```sh
# on server
cc-server --db <path-to-db> --url <url-to-serve> --index <path-to-index.html>
```

then visit `<url>` in your browser to see the index page. 
an example is provided at [example_index.html](./example_index.html):
```sh
# in the project root
cargo run --bin cc-server -- --db ./cc.db --index ./example_index.html   
```

```sh
# on client

# put --> {"ok":true,"msg":"<code>"}
curl -X POST <url>/put -d '<long-url>'

# get --> redirects to <long-url>
curl -i <url>/<code>
```

## api
- `POST /put` with body being a url to shorten
- will return a json object with:
  - `ok`: did it work (or check the status code; will be 201, 400, or 500)
  - `msg`: the code for the url if `ok`, otherwise an error message to display to the user
- `GET /<code>` will 308 to the original url if the code exists, or 404
- `GET /` serves the index page if specified, or 404