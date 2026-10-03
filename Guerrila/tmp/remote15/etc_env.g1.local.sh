export HELIUS_RPC_URL='https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY'
export HELIUS_GATEKEEPER='https://beta.helius-rpc.com/?api-key=REDACTED_API_KEY'
export SHADOW_RPC_URL='https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY'
export SHADOW_BOOTSTRAP_RPC_URL='https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY'
export SHADOW_SOLANA_RPC_URL='https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY'
export SHADOW_PUBLIC_RPC_URL='https://beta.helius-rpc.com/?api-key=REDACTED_API_KEY'
export SHADOW_LOCAL_AGAVE_REFERENCE_RPC_URL='https://mainnet.helius-rpc.com/?api-key=REDACTED_API_KEY'
export SHADOW_LOCAL_AGAVE_CPU_AFFINITY='0-7,14-31,38-47'
export SH1_VALIDATOR_CPU_AFFINITY='0-7,14-31,38-47'
export SH1_REPLAY_TRANSACTIONS_THREADS=36
export SH1_RAYON_GLOBAL_THREADS=36
export HELIUS_GATEKEEPER_URL='https://beta.helius-rpc.com/?api-key=REDACTED_API_KEY'
export HELIUS_LASERSTREAM_URL='https://laserstream-mainnet-ewr.helius-rpc.com'
export HELIUS_LASERSTREAM_API_KEY='REDACTED_CREDENTIAL'
export SHADOW_GRPC_SOURCE_GRADE='laserstream'
export SH1_ONLY_KNOWN_RPC='false'

# Explicit Agave validator bind address for .15 startup port checks.
export SH1_BIND_ADDRESS='109.94.99.15'

# Bypass Agave startup UDP echo gate; host firewall is open and this local source is non-voting.
export SH1_NO_PORT_CHECK='true'

export SH1_MAXIMUM_LOCAL_SNAPSHOT_AGE='200'
