#!/bin/bash

echo "🚀 Starting Kolme with Real ZKP Implementation..."
echo "================================================"

# Load environment variables
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

# Check if ZKP parameters exist
if [ ! -f "packages/zkp-auth/data/proving_key.bin" ] || [ ! -f "packages/zkp-auth/data/verifying_key.bin" ]; then
    echo "❌ ZKP parameters not found. Please run setup first:"
    echo "   ./scripts/setup-zkp-real.sh"
    exit 1
fi

echo "✅ ZKP parameters found"
echo "📊 Parameter hash: $(cat packages/zkp-auth/data/parameters.hash)"

# Set ZKP environment variables
export ZKP_ENABLED=true
export ZKP_PARAMS_DIR="$(pwd)/packages/zkp-auth/data"
export RUST_LOG=${RUST_LOG:-info}

echo "🔧 Configuration:"
echo "  - ZKP Enabled: $ZKP_ENABLED"
echo "  - Parameters: $ZKP_PARAMS_DIR"
echo "  - Log Level: $RUST_LOG"
echo "  - API Port: ${KOLME_API_PORT:-8080}"

# Check if OAuth credentials are configured
if [ "$GITHUB_CLIENT_ID" = "your_github_client_id" ]; then
    echo "⚠️  Warning: OAuth credentials not configured in .env file"
    echo "The system will start but OAuth login won't work properly."
    echo ""
fi

# Start Kolme server
echo ""
echo "🌐 Starting Kolme API server on port ${KOLME_API_PORT:-8080}..."
echo "   Health check: http://localhost:${KOLME_API_PORT:-8080}/health"
echo "   ZKP endpoints: http://localhost:${KOLME_API_PORT:-8080}/api/zkp"
echo ""

cd packages/kolme
cargo run --release
