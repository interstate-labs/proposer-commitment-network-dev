# proposer-commitment-network

![Version: 0.1.0](https://img.shields.io/badge/Version-0.1.0-informational?style=flat-square) ![Type: application](https://img.shields.io/badge/Type-application-informational?style=flat-square) ![AppVersion: 1.16.0](https://img.shields.io/badge/AppVersion-1.16.0-informational?style=flat-square)

A Helm chart for Interstate commit-boost and sidecar components

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| appName | string | `"proposer-commitment-network"` | The name of the application, used as a prefix for component names |
| cb.annotations | object | `{}` | Additional annotations for the CB component |
| cb.config.chain | string | `"Holesky"` | Blockchain network to connect to |
| cb.config.logs.logDirPath | string | `"./logs"` | Directory path for log files |
| cb.config.logs.logLevel | string | `"debug"` | Log level (debug, info, warn, error) |
| cb.config.logs.maxLogFiles | int | `30` | Maximum number of log files to retain |
| cb.config.metrics.prometheusConfig | string | `"./prometheus.yml"` | Path to Prometheus configuration file |
| cb.config.pbs.beaconRpc | string | `"http://5.161.69.231:32781"` | URL for the beacon node RPC |
| cb.config.pbs.genesisTimeSec | string | `"1738648239"` | Genesis timestamp in seconds since Unix epoch |
| cb.config.pbs.host | string | `"0.0.0.0"` | Host address to bind to |
| cb.config.relays[0] | object | `{"url":"https://0x821f2a65afb70e7f2e820a925a9b4c80a159620582c1766b1b09729fec178b11ea22abb3a51f07b288be815a1a2ff516@bloxroute.holesky.blxrbdn.com"}` | List of relay endpoints to connect to |
| cb.deployment.affinity | object | `{"podAntiAffinity":{"preferredDuringSchedulingIgnoredDuringExecution":[{"podAffinityTerm":{"labelSelector":{"matchExpressions":[{"key":"app.kubernetes.io/name","operator":"In","values":["propose-commitment-network"]},{"key":"app.kubernetes.io/component","operator":"In","values":["cb"]}]},"topologyKey":"kubernetes.io/hostname"},"weight":100}]}}` | Pod affinity rules for scheduling |
| cb.deployment.containerSecurityContext | object | `{"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]},"readOnlyRootFilesystem":true}` | Security context for the container |
| cb.deployment.containerSecurityContext.allowPrivilegeEscalation | bool | `false` | Prevent privilege escalation |
| cb.deployment.containerSecurityContext.capabilities.drop | list | `["ALL"]` | Drop all Linux capabilities for security |
| cb.deployment.containerSecurityContext.readOnlyRootFilesystem | bool | `true` | Mount the root filesystem as read-only |
| cb.deployment.logLevel | string | `"debug"` | Log level for the deployment |
| cb.deployment.podSecurityContext | object | `{"fsGroup":1000,"runAsNonRoot":true,"runAsUser":1000}` | Security context for the pod |
| cb.deployment.podSecurityContext.fsGroup | int | `1000` | Group ID to run the pod as |
| cb.deployment.podSecurityContext.runAsNonRoot | bool | `true` | Run containers as non-root |
| cb.deployment.podSecurityContext.runAsUser | int | `1000` | User ID to run the container as |
| cb.deployment.port | int | `18550` | Port for the CB service |
| cb.deployment.replicas | int | `1` | Number of replicas for the deployment |
| cb.deployment.resources | object | `{"limits":{"cpu":"1000m","memory":"1Gi"},"requests":{"cpu":"200m","memory":"256Mi"}}` | Resource limits and requests for the container |
| cb.deployment.resources.limits.cpu | string | `"1000m"` | CPU limit |
| cb.deployment.resources.limits.memory | string | `"1Gi"` | Memory limit |
| cb.deployment.resources.requests.cpu | string | `"200m"` | CPU request |
| cb.deployment.resources.requests.memory | string | `"256Mi"` | Memory request |
| cb.deployment.terminationGracePeriodSeconds | int | `60` | Termination grace period in seconds |
| cb.deployment.updateStrategy | object | `{"maxSurge":1,"maxUnavailable":0,"type":"RollingUpdate"}` | Update strategy configuration for the deployment |
| cb.deployment.updateStrategy.maxSurge | int | `1` | Maximum number of pods that can be created over the desired number of pods |
| cb.deployment.updateStrategy.maxUnavailable | int | `0` | Maximum number of pods that can be unavailable during the update |
| cb.deployment.updateStrategy.type | string | `"RollingUpdate"` | Type of update strategy (RollingUpdate or Recreate) |
| cb.enabled | bool | `true` | Enable or disable the CB component |
| cb.image.pullPolicy | string | "IfNotPresent" | Image pull policy |
| cb.image.repository | string | `"interstatecrypto/interstate-pbs-module"` | Docker image repository for the CB component |
| cb.image.tag | string | "latest" | Docker image tag for the CB component |
| cb.port | int | `18550` | Port for the CB service |
| sidecar | object | `{"deployment":{"containerSecurityContext":{"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}},"livenessProbe":{"httpGet":{"path":"/health","port":"metrics"},"initialDelaySeconds":30,"periodSeconds":10},"podSecurityContext":{"fsGroup":1000,"runAsNonRoot":true,"runAsUser":1000},"readinessProbe":{"httpGet":{"path":"/ready","port":"metrics"},"initialDelaySeconds":5,"periodSeconds":10},"replicas":1,"resources":{"limits":{"cpu":"1000m","memory":"1Gi"},"requests":{"cpu":"200m","memory":"256Mi"}},"terminationGracePeriodSeconds":30,"updateStrategy":{"maxSurge":1,"maxUnavailable":0,"type":"RollingUpdate"}},"enabled":true,"env":{"beaconApiUrl":"http://5.161.69.231:32781","builderPort":"9064","caCertPath":"/root/kartos/web3signer-25.2.0/crt/w3s.crt","cargoManifestDir":"/app","chain":"kurtosis","clientCombinedPemPath":"/root/kartos/web3signer-25.2.0/crt/my_cert.pem","collectorSocket":"ws://5.161.69.231:4000/ws","collectorUrl":"http://95.216.145.221:18550","commitBoostSignerUrl":"http://5.161.69.231:18551","commitmentDeadline":"100","commitmentPort":"9063","delegationsPath":"/app/delegations/delegations.json","engineApiUrl":"http://5.161.69.231:32771","executionApiUrl":"http://5.161.69.231:32773","feeRecipient":"0x8aC112a5540f441cC9beBcC647041A6E0D595B94","gatewayContract":"0x6db20C530b3F96CD5ef64Da2b1b931Cb8f264009","jwt":"dc49981516e8e72b401a63e6405495a32dafc3939b5d6d83cc319ac0388bca1b","metricsPort":"8018","rustBacktrace":"1","rustLog":"debug","sidecarInfoSenderUrl":"http://5.161.69.231:8000","slotTime":"2","validatorIndexes":"0..64","web3SignerUrl":"https://b2e4-2a01-4ff-f0-4039-00-1.ngrok-free.app"},"image":{"pullPolicy":"IfNotPresent","repository":"interstatecrypto/interstate-sidecar","tag":"latest"},"ingress":{"annotations":{"kubernetes.io/ingress.class":"nginx","nginx.ingress.kubernetes.io/rewrite-target":"/"},"className":"nginx","enabled":false,"hosts":[{"host":null,"paths":[{"path":"/","pathType":"Prefix","serviceName":"commitment"}]}],"tls":[{"hosts":null,"secretName":"sidecar-tls-secret"}]},"service":{"annotations":{},"exposeMetrics":false,"nodePorts":{"commitment":null},"type":"NodePort"}}` | Enable or disable the sidecar component |
| sidecar.deployment.containerSecurityContext | object | `{"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}}` | Security context for the container |
| sidecar.deployment.containerSecurityContext.allowPrivilegeEscalation | bool | `false` | Prevent privilege escalation |
| sidecar.deployment.containerSecurityContext.capabilities.drop | list | `["ALL"]` | Drop all Linux capabilities for security |
| sidecar.deployment.livenessProbe | object | `{"httpGet":{"path":"/health","port":"metrics"},"initialDelaySeconds":30,"periodSeconds":10}` | Liveness probe configuration to determine if the container is running |
| sidecar.deployment.livenessProbe.httpGet.path | string | `"/health"` | Path for liveness probe |
| sidecar.deployment.livenessProbe.httpGet.port | string | `"metrics"` | Port for liveness probe |
| sidecar.deployment.livenessProbe.initialDelaySeconds | int | `30` | Initial delay before probing starts |
| sidecar.deployment.livenessProbe.periodSeconds | int | `10` | How often to perform the probe |
| sidecar.deployment.podSecurityContext | object | `{"fsGroup":1000,"runAsNonRoot":true,"runAsUser":1000}` | Security context for the pod |
| sidecar.deployment.podSecurityContext.fsGroup | int | `1000` | Group ID to run the pod as |
| sidecar.deployment.podSecurityContext.runAsNonRoot | bool | `true` | Run containers as non-root |
| sidecar.deployment.podSecurityContext.runAsUser | int | `1000` | User ID to run the container as |
| sidecar.deployment.readinessProbe | object | `{"httpGet":{"path":"/ready","port":"metrics"},"initialDelaySeconds":5,"periodSeconds":10}` | Readiness probe configuration to determine if the container is ready to receive traffic |
| sidecar.deployment.readinessProbe.httpGet.path | string | `"/ready"` | Path for readiness probe |
| sidecar.deployment.readinessProbe.httpGet.port | string | `"metrics"` | Port for readiness probe |
| sidecar.deployment.readinessProbe.initialDelaySeconds | int | `5` | Initial delay before probing starts |
| sidecar.deployment.readinessProbe.periodSeconds | int | `10` | How often to perform the probe |
| sidecar.deployment.replicas | int | `1` | Number of replicas for the deployment |
| sidecar.deployment.resources | object | `{"limits":{"cpu":"1000m","memory":"1Gi"},"requests":{"cpu":"200m","memory":"256Mi"}}` | Resource limits and requests for the container |
| sidecar.deployment.resources.limits.cpu | string | `"1000m"` | CPU limit |
| sidecar.deployment.resources.limits.memory | string | `"1Gi"` | Memory limit |
| sidecar.deployment.resources.requests.cpu | string | `"200m"` | CPU request |
| sidecar.deployment.resources.requests.memory | string | `"256Mi"` | Memory request |
| sidecar.deployment.terminationGracePeriodSeconds | int | `30` | Termination grace period in seconds |
| sidecar.deployment.updateStrategy | object | `{"maxSurge":1,"maxUnavailable":0,"type":"RollingUpdate"}` | Update strategy configuration for the deployment |
| sidecar.deployment.updateStrategy.maxSurge | int | `1` | Maximum number of pods that can be created over the desired number of pods |
| sidecar.deployment.updateStrategy.maxUnavailable | int | `0` | Maximum number of pods that can be unavailable during the update |
| sidecar.deployment.updateStrategy.type | string | `"RollingUpdate"` | Type of update strategy (RollingUpdate or Recreate) |
| sidecar.env.beaconApiUrl | string | `"http://5.161.69.231:32781"` | URL for the beacon node API |
| sidecar.env.builderPort | string | `"9064"` | Port for the builder service |
| sidecar.env.caCertPath | string | `"/root/kartos/web3signer-25.2.0/crt/w3s.crt"` | Path to CA certificate file |
| sidecar.env.cargoManifestDir | string | `"/app"` | Path to the cargo manifest directory |
| sidecar.env.chain | string | `"kurtosis"` | Blockchain network to connect to |
| sidecar.env.clientCombinedPemPath | string | `"/root/kartos/web3signer-25.2.0/crt/my_cert.pem"` | Path to combined client certificate and key PEM file |
| sidecar.env.collectorSocket | string | `"ws://5.161.69.231:4000/ws"` | WebSocket URL for the collector service |
| sidecar.env.collectorUrl | string | `"http://95.216.145.221:18550"` | URL for the collector service |
| sidecar.env.commitBoostSignerUrl | string | `"http://5.161.69.231:18551"` | URL for commit boost signer service |
| sidecar.env.commitmentDeadline | string | `"100"` | Deadline for commitments in milliseconds |
| sidecar.env.commitmentPort | string | `"9063"` | Port for commitment service |
| sidecar.env.delegationsPath | string | `"/app/delegations/delegations.json"` | Path to delegations configuration file |
| sidecar.env.engineApiUrl | string | `"http://5.161.69.231:32771"` | URL for the engine API |
| sidecar.env.executionApiUrl | string | `"http://5.161.69.231:32773"` | URL for the execution client API |
| sidecar.env.feeRecipient | string | `"0x8aC112a5540f441cC9beBcC647041A6E0D595B94"` | Ethereum address to receive fees |
| sidecar.env.gatewayContract | string | `"0x6db20C530b3F96CD5ef64Da2b1b931Cb8f264009"` | Ethereum address of the gateway contract |
| sidecar.env.jwt | string | `"dc49981516e8e72b401a63e6405495a32dafc3939b5d6d83cc319ac0388bca1b"` | JWT token for authentication |
| sidecar.env.metricsPort | string | `"8018"` | Port for metrics service |
| sidecar.env.rustBacktrace | string | `"1"` | Enable Rust backtrace (1=enabled, 0=disabled) |
| sidecar.env.rustLog | string | `"debug"` | Rust logging level |
| sidecar.env.sidecarInfoSenderUrl | string | `"http://5.161.69.231:8000"` | URL for sidecar info sender service |
| sidecar.env.slotTime | string | `"2"` | Time in seconds between slots |
| sidecar.env.validatorIndexes | string | `"0..64"` | Range of validator indexes to monitor |
| sidecar.env.web3SignerUrl | string | `"https://b2e4-2a01-4ff-f0-4039-00-1.ngrok-free.app"` | URL for the Web3Signer service |
| sidecar.image.pullPolicy | string | "IfNotPresent" | Image pull policy |
| sidecar.image.repository | string | `"interstatecrypto/interstate-sidecar"` | Docker image repository for the sidecar component |
| sidecar.image.tag | string | "latest" | Docker image tag for the sidecar component |
| sidecar.ingress.annotations | object | `{"kubernetes.io/ingress.class":"nginx","nginx.ingress.kubernetes.io/rewrite-target":"/"}` | Additional annotations for the ingress |
| sidecar.ingress.className | string | `"nginx"` | Ingress class name to use |
| sidecar.ingress.enabled | bool | `false` | Enable or disable the ingress |
| sidecar.ingress.hosts | list | `[{"host":null,"paths":[{"path":"/","pathType":"Prefix","serviceName":"commitment"}]}]` | List of host configurations for the ingress |
| sidecar.ingress.hosts[0].paths[0].pathType | string | `"Prefix"` | Path type for the ingress (Prefix, Exact, ImplementationSpecific) |
| sidecar.ingress.hosts[0].paths[0].serviceName | string | `"commitment"` | Service port name to route traffic to |
| sidecar.ingress.tls | list | `[{"hosts":null,"secretName":"sidecar-tls-secret"}]` | TLS configuration for the ingress |
| sidecar.service.annotations | object | `{}` | Additional annotations for the service |
| sidecar.service.exposeMetrics | bool | false | Whether to expose the metrics port when using NodePort service type |
| sidecar.service.nodePorts | object | `{"commitment":null}` | NodePort specifications for service ports |
| sidecar.service.nodePorts.commitment | string | `nil` | Specify commitment port number (30000-32767) or leave null for auto-assignment |
| sidecar.service.type | string | NodePort | Service type, values: NodePort, ClusterIP, LoadBalancer |

----------------------------------------------

