# Interstate Boost Kustomize Setup

## Requirements

This setup requires [kubectl](https://kubernetes.io/docs/reference/kubectl/) with [kustomize](https://kustomize.io/) installed.

### Configuration Requirements

This kustomize configuration requires:

* Holesky Execution RPC. Ie. `http://execution:8545`
* Holesky Engine endpoint. Ie. `http://execution:8551`
* Holesky Beacon API. Ie. `http://beacon:5052`
* Holesky Dirk endpoint. `http://dirk:8881`
* External secrets store configured. This is required if you are storing secrets outside the Kubernetes cluster(ie. AWS SM or similar).

## Details

This configuration uses the [generic-app](https://github.com/NethermindEth/helm-charts/tree/main/charts/generic-app) chart which essentially deploys a custom application based on the configured `values.yaml`. Because of that most of the configurations are inside the provided [values.yaml](./values.yaml).

The setup consist of a deployment running 3 containers with 1 initialization container:

1. Main container for `interstate-cb-module`
2. Extra container for `commit-boost-signer`
3. Extra container for `interstate-sidecar`

## Steps to deploy

Before deploying make sure to update the [values.yaml](./values.yaml) with the correct configuration. Most of the configurations can be done from the environment variables of the main, extra and initialization containers.

### Deployment

The deployment can be done using the `kustomize` tool. For that you can use the following command:

```bash
kubectl kustomize . --enable-helm | kubectl apply -f -
```
