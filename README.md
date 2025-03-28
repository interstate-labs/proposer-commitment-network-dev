[![Docs](https://img.shields.io/badge/docs-latest-blue.svg)](docs.interstate.so)
[![Chat](https://img.shields.io/endpoint?color=neon&logo=telegram&label=chat&url=https%3A%2F%2Ftg.sumanjay.workers.dev%2F%2BPcs9bykxK3BiMzk5)]([https://t.me/+Pcs9bykxK3BiMzk5](https://t.me/+-i4dP7U2BggxMzAx))
[![X](https://img.shields.io/twitter/follow/interstatefdn)](https://x.com/interstatefdn)

# Interstate Sidecar To Enable Continuous Transaction Execution on Mainnet.
Interstate is an extension to the PBS / MEV-Boost pipeline which enables instant and continuous transaction confirmations on mainnet, this is a massive UX improvement for Ethereum. 

![Full Design](static/flow.jpg)

We follow the common api preconfirmation api spec. Read the full docs at: https://docs.interstate.so

# Setting up Web3Signer
Interstate sidecar should be run in a secure enclave, without access to keystores or any other sensitive data. We require running web3signer or another signer in order to create this enclave. Please see https://docs.interstate.so for further details.
