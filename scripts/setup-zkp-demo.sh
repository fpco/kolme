#!/bin/bash

# Simplified setup script for ZKP demo

set -e

echo "🔐 Kolme ZKP Demo Setup (Simplified)"
echo "===================================="
echo ""

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "packages/zkp-auth" ]; then
    echo "❌ Error: Please run this script from the Kolme project root"
    exit 1
fi

# Create mock parameters directory
echo "📊 Creating mock parameter files for demo..."
mkdir -p packages/zkp-auth/data

# Create mock proving key (just for demo)
echo "Creating mock proving_key.bin..."
dd if=/dev/urandom of=packages/zkp-auth/data/proving_key.bin bs=1024 count=10 2>/dev/null

# Create mock verifying key (just for demo)
echo "Creating mock verifying_key.bin..."
dd if=/dev/urandom of=packages/zkp-auth/data/verifying_key.bin bs=1024 count=1 2>/dev/null

# Create parameter hash
echo "Creating parameters.hash..."
echo "mock-parameter-hash-for-demo" > packages/zkp-auth/data/parameters.hash

# Create .env file if it doesn't exist
if [ ! -f ".env" ]; then
    cat > .env << EOF
# OAuth Credentials (use mock values for demo)
TWITTER_CLIENT_ID=mock_twitter_client_id
TWITTER_CLIENT_SECRET=mock_twitter_client_secret

GITHUB_CLIENT_ID=mock_github_client_id
GITHUB_CLIENT_SECRET=mock_github_client_secret

DISCORD_CLIENT_ID=mock_discord_client_id
DISCORD_CLIENT_SECRET=mock_discord_client_secret

GOOGLE_CLIENT_ID=mock_google_client_id
GOOGLE_CLIENT_SECRET=mock_google_client_secret

# Kolme Configuration
KOLME_ZKP_PARAMS_DIR=./packages/zkp-auth/data
KOLME_API_PORT=8080

# Database
DATABASE_URL=sqlite:./kolme.db
EOF
    echo "✅ Created .env file with mock credentials."
else
    echo "✅ .env file already exists."
fi

# Create simplified start script
cat > start-demo-simple.sh << 'EOF'
#!/bin/bash

echo "🚀 Starting ZKP Login Demo (Simplified)..."
echo ""
echo "Note: This uses the mock client for demonstration."
echo "To use real ZKP, you need to properly build the zkp-auth package."
echo ""

cd demo/zkp-login-demo

# Check if node_modules exists
if [ ! -d "node_modules" ]; then
    echo "📦 Installing dependencies..."
    npm install
fi

echo "🌐 Starting demo server..."
npm run dev
EOF

chmod +x start-demo-simple.sh

echo ""
echo "✨ Simplified Setup Complete!"
echo "============================="
echo ""
echo "To run the demo:"
echo "1. Run: ./start-demo-simple.sh"
echo "2. Open http://localhost:3000 in your browser"
echo ""
echo "⚠️  Note: This simplified setup uses mock parameters."
echo "For real cryptographic proofs, you need to:"
echo "1. Fix the ark-sponge dependency issues"
echo "2. Build the setup binary: cargo build --bin setup"
echo "3. Run the full setup script"
echo ""
echo "The demo will still show the full flow, but with simulated proofs."