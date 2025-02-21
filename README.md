[![Docs](https://img.shields.io/badge/docs-latest-blue.svg)](docs.interstate.so)
[![Chat](https://img.shields.io/endpoint?color=neon&logo=telegram&label=chat&url=https%3A%2F%2Ftg.sumanjay.workers.dev%2F%2BPcs9bykxK3BiMzk5)]([https://t.me/+Pcs9bykxK3BiMzk5](https://t.me/+-i4dP7U2BggxMzAx))
[![X](https://img.shields.io/twitter/follow/interstatefdn)](https://x.com/interstatefdn)

# Interstate Sidecar To Enable Continuous Transaction Execution on Mainnet.
Interstate is an extension to the PBS / MEV-Boost pipeline which enables instant and continuous transaction confirmations on mainnet, this is a massive UX improvement for Ethereum. 

![Full Design](static/flow.jpg)

We follow the common api preconfirmation api spec. Read the full docs at: https://docs.interstate.so

# Web3Signer Setting up.
1. Download and unzip web3signer package.
wget https://artifacts.consensys.net/public/web3signer/raw/names/web3signer.tar.gz/versions/latest/web3signer.tar.gz
tar -xvzf web3signer.tar.gz
2. Make the web3signer command as the system command.
echo 'export PATH=$PATH:/home/web3signer-25.2.0/bin'>> ~/.bashrc 
source ~/.bashrc
echo 'export PATH=$PATH:/home/web3signer-25.2.0/bin'>> ~/.zshrc 
source ~/.zshrc
3. [clone ](https://github.com/voldev94321/copying-validator-keystores) and copy the keystores and other files needs for web3signer.
4. Start the web3signer server.
web3signer --tls-allow-any-client true --tls-keystore-file /home/web3signer-25.2.0/tls/key.p12 --tls-keystore-password-file /home/web3signer-25.2.0/tls/password.txt eth2 --network mainnet --keystores-path /home/web3signer-25.2.0/keystore/keys  --keystores-passwords-path /home/web3signer-25.2.0/keystore/secrets --slashing-protection-enabled false --commit-boost-api-enabled true --proxy-keystores-path /home/web3signer-25.2.0/tls/keystore --proxy-keystores-password-file /home/web3signer-25.2.0/tls/password.txt