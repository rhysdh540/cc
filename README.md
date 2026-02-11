# cc

makes urls short

## usage

```sh
# on server
cc-server --db <path-to-db> --url <url-to-serve>
```

```sh
# on client

# put --> {"ok":true,"msg":"<code>"}
curl -X POST <url>/put -d '<long-url>'

# get --> redirects to <long-url>
curl -i <url>/<code>
```