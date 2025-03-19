# Get Started

## How to Install Helm

    $ curl -fsSL -o get_helm.sh https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3
    $ chmod 700 get_helm.sh
    $ ./get_helm.sh

## How to install Docker

    https://www.digitalocean.com/community/tutorials/how-to-install-and-use-docker-on-ubuntu-20-04

## How to install Kubectl

### Update system packages

    sudo apt update && sudo apt upgrade -y  # Debian/Ubuntu
    sudo yum update -y                      # RHEL/CentOS

### Download and install kubectl

    curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl"
    chmod +x kubectl
    sudo mv kubectl /usr/local/bin/

## How to work with helm

### Create the new helm

    helm create <namespace>

### Create the package 

    helm package <namespace>

### Update yaml

Please update values.yaml, configMap.yaml, deployment.yaml and service.yaml files

### How to set env to connect sidecar to external signer

    https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/values.yaml#L61C1-L61C3

    https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/templates/configMap.yaml#L55

    Here, please update the COMMIT_BOOST_SIGNER_URL with external signer link

### How to run

    helm install <namespace> ./<namespace>

    eg: helm install proposer-commitment-network ./proposer-commitment-network

## How to set envs for interstate-cb module

    In values.yaml

        `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/values.yaml#L26` and `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/values.yaml#L28`

        `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/values.yaml#L83` and `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/values.yaml#L89`

        Please update the CHAIN Name and BEACON_RPC

    In templates/configMap.yaml

        `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/templates/configMap.yaml#L7` and `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/templates/configMap.yaml#L13`

        Please update the CHAIN Name and BEACON_RPC

## How to set envs for interstate-sidecar

    In values.yaml

        From `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/values.yaml#L37` to `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/values.yaml#L61`

        Please update envs for interstate-sidecar

    In templates/configMap.yaml
    
        From `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/templates/configMap.yaml#L32` to `https://github.com/interstate-labs/proposer-commitment-network-dev/blob/feat/helm/proposer-commitment-network/templates/configMap.yaml#L55`

        Please update envs for interstate-sidecar

## Check the running kubnets

    kubectl get pods

## How to forward port into local

    kubectl port-forward svc/propose-commitment-network-service 9063:9063

## How to fix if you see this error

    if you have facing this type problem
    launch ec2-instance with t2.medium and extend storage capacity

    E0919 14:57:21.242964 414467 memcache.go:265] couldn’t get current server API group list: Get “http://localhost:8080/api?timeout=32s ”: dial tcp 127.0.0.1:8080: connect: connection refused

    step 1
        apt update

    step 2
        apt install docker.io

    step3
        curl -LO https://storage.googleapis.com/minikube/releases/latest/minikube-linux-amd64
        sudo install minikube-linux-amd64 /usr/local/bin/minikube && rm minikube-linux-amd64

    step 4
        minikube start --force

    step 5
        kubectl get nodes
