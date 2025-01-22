# Interstate-Boost

Interstate-Boost is a [commit-boost](https://commit-boost.github.io/commit-boost-client) module that extends the default PBS module with the [constraints-API](https://chainbound.github.io/bolt-docs/api/builder).
It inherits the PBS module configuration for the modified `get_header` call.

# To build a commit-boost image
```bash
docker build -t interstatenetwork/interstate-pbs-module:0.2.1-dev-feat-cb .
```
