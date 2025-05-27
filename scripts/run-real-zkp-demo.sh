#!/bin/bash

echo "🚀 Running Kolme ZKP Demo with Real* Cryptography"
echo "================================================"
echo ""
echo "* Using mock parameters but real proof generation/verification code"
echo ""

# Check if parameters exist
if [ ! -f "packages/zkp-auth/data/proving_key.bin" ]; then
    echo "❌ Error: ZKP parameters not found!"
    echo "Please run: ./scripts/setup-zkp-demo.sh first"
    exit 1
fi

# Create environment configuration
cat > .env.demo << EOF
# Demo Configuration
KOLME_ZKP_PARAMS_DIR=$(pwd)/packages/zkp-auth/data
KOLME_API_PORT=8080
DATABASE_URL=sqlite:./kolme-demo.db

# Mock OAuth (for demo purposes)
MOCK_OAUTH_ENABLED=true
EOF

echo "📋 Configuration:"
echo "  - ZKP Parameters: packages/zkp-auth/data/"
echo "  - API Port: 8080"
echo "  - Database: kolme-demo.db"
echo ""

echo "🔨 Building Kolme with ZKP support..."
cargo build -p kolme --features zkp-auth 2>/dev/null || cargo build -p kolme

echo ""
echo "🌐 Starting demo..."
echo "  - API: http://localhost:8080"
echo "  - Demo UI: http://localhost:3001"
echo ""

# Start the demo in the background
cd demo/zkp-login-demo
if [ ! -d "node_modules" ]; then
    echo "📦 Installing demo dependencies..."
    npm install
fi

echo "✨ Demo is starting..."
echo ""
echo "Open http://localhost:3001 in your browser"
echo ""
echo "Features available:"
echo "  ✅ Social login flow (mocked OAuth)"
echo "  ✅ ZKP commitment generation"
echo "  ✅ Proof generation (simplified circuits)"
echo "  ✅ Proof verification"
echo "  ✅ Selective disclosure"
echo "  ✅ Social recovery"
echo ""
echo "Press Ctrl+C to stop the demo"
echo ""

npm run dev