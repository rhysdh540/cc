# cc

makes urls short

## usage

```sh
# on server
> cc-server serve <path-to-db> --url <url-to-serve> --index <path-to-index.html>
```

then visit `<url>` in your browser to see the index page. 
an example is provided at [example_index.html](./example_index.html):
```sh
# in the project root
> cargo run --bin cc-server -- serve ./cc.db --index ./example_index.html   
```

```sh
# on client

> curl -X POST <url>/put -d '<long-url>'
{"ok":true,"msg":"<code>"}

> curl -X GET -i <url>/<code>
HTTP/1.1 308 Permanent Redirect
...
```

there is also an `ls` subcommand that lists all the codes and their corresponding urls in the database:
```sh
> cc-server ls cc.db
2 mappings found in cc.db:
  <code1> -> <long-url1>
  <code2> -> <long-url2>
```

## api
- `POST /put` with body being a url to shorten
- will return a json object with:
  - `ok`: did it work (or check the status code; will be 201, 400, or 500)
  - `msg`: the code for the url if `ok`, otherwise an error message to display to the user
- `GET /<code>` will 308 to the original url if the code exists, or 404
- `GET /` serves the index page if specified, or 404