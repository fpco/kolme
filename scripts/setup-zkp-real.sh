#!/bin/bash

# Kolme ZKP Real Implementation Setup Script
# This script sets up the complete ZKP system with real cryptographic operations

set -e

echo "🔐 Kolme ZKP Real Implementation Setup"
echo "======================================"
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "packages/zkp-auth" ]; then
    print_error "Please run this script from the Kolme project root directory"
    exit 1
fi

print_status "Setting up ZKP Real Implementation..."

# Step 1: Generate real ZKP parameters
print_status "Step 1: Generating ZKP trusted setup parameters..."
cd packages/zkp-auth

if [ -f "data/proving_key.bin" ] && [ -f "data/verifying_key.bin" ]; then
    print_warning "ZKP parameters already exist. Regenerating..."
    rm -rf data/
fi

print_status "Running trusted setup (this may take a few seconds)..."
cargo run --bin setup --release

if [ ! -f "data/proving_key.bin" ] || [ ! -f "data/verifying_key.bin" ]; then
    print_error "Failed to generate ZKP parameters"
    exit 1
fi

print_success "ZKP parameters generated successfully"
print_status "  - Proving key: $(du -h data/proving_key.bin | cut -f1)"
print_status "  - Verifying key: $(du -h data/verifying_key.bin | cut -f1)"
print_status "  - Parameter hash: $(cat data/parameters.hash | head -c 16)..."

cd ../..

# Step 2: Set up OAuth credentials
echo ""
echo "🔑 Step 2: OAuth Configuration"
echo "You need to create OAuth applications on each platform:"
echo ""
echo "1. Twitter: https://developer.twitter.com/apps"
echo "2. GitHub: https://github.com/settings/developers"
echo "3. Discord: https://discord.com/developers/applications"
echo "4. Google: https://console.cloud.google.com/"
echo ""
echo "For each platform, set the redirect URI to: http://localhost:3000/callback"
echo ""

# Create .env file if it doesn't exist
if [ ! -f ".env" ]; then
    cat > .env << EOF
# OAuth Credentials
TWITTER_CLIENT_ID=your_twitter_client_id
TWITTER_CLIENT_SECRET=your_twitter_client_secret

GITHUB_CLIENT_ID=your_github_client_id
GITHUB_CLIENT_SECRET=your_github_client_secret

DISCORD_CLIENT_ID=your_discord_client_id
DISCORD_CLIENT_SECRET=your_discord_client_secret

GOOGLE_CLIENT_ID=your_google_client_id
GOOGLE_CLIENT_SECRET=your_google_client_secret

# Kolme Configuration
KOLME_ZKP_PARAMS_DIR=./packages/zkp-auth/data
KOLME_API_PORT=8080

# Database
DATABASE_URL=sqlite:./kolme.db
EOF
    echo "✅ Created .env file. Please edit it with your OAuth credentials."
else
    echo "✅ .env file already exists."
fi

# Step 3: Build the system
echo ""
echo "🔨 Step 3: Building Kolme with ZKP support..."
cargo build --release -p kolme

# Step 4: Run database migrations
echo ""
echo "📊 Step 4: Setting up database..."
export DATABASE_URL="sqlite:./kolme.db"
cargo sqlx database create || true
cargo sqlx migrate run --source packages/kolme/migrations

# Step 5: Create startup script
echo ""
echo "🚀 Step 5: Creating startup script..."

cat > start-kolme-zkp.sh << 'EOF'
#!/bin/bash

# Load environment variables
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

# Check if OAuth credentials are configured
if [ "$GITHUB_CLIENT_ID" = "your_github_client_id" ]; then
    echo "⚠️  Warning: OAuth credentials not configured in .env file"
    echo "The system will start but OAuth login won't work properly."
    echo ""
fi

# Start Kolme with ZKP support
echo "🚀 Starting Kolme with ZKP authentication..."
echo "API will be available at: http://localhost:${KOLME_API_PORT:-8080}"
echo ""

cargo run --release -p kolme -- \
    --zkp-enabled \
    --zkp-params-dir "${KOLME_ZKP_PARAMS_DIR:-./packages/zkp-auth/data}" \
    --api-port "${KOLME_API_PORT:-8080}"
EOF

chmod +x start-kolme-zkp.sh

# Step 6: Create demo startup script
cat > start-demo-real.sh << 'EOF'
#!/bin/bash

# Start the real demo (not mocked)
echo "🌐 Starting real ZKP demo..."
echo ""

# First, ensure Kolme is running
if ! lsof -i:8080 > /dev/null 2>&1; then
    echo "⚠️  Kolme is not running on port 8080"
    echo "Please run ./start-kolme-zkp.sh in another terminal first"
    exit 1
fi

# Update demo to use real client
cd demo/zkp-login-demo

# Replace mock client with real client
if [ -f "src/lib/mockClient.ts" ]; then
    mv src/lib/mockClient.ts src/lib/mockClient.ts.bak
fi

cat > src/lib/client.ts << 'TYPESCRIPT'
import { KolmeZKPClient } from '@kolme/zkp-sdk';

export const kolmeClient = new KolmeZKPClient({
  apiUrl: process.env.VITE_KOLME_API_URL || 'http://localhost:8080',
});

// Re-export for compatibility
export { kolmeClient as mockKolmeClient };
TYPESCRIPT

# Create environment file
cat > .env.local << 'ENV'
VITE_KOLME_API_URL=http://localhost:8080
ENV

# Install and run
npm install
npm run dev
EOF

chmod +x start-demo-real.sh

# Final instructions
echo ""
echo "✨ Setup Complete!"
echo "=================="
echo ""
echo "To run the real ZKP authentication system:"
echo ""
echo "1. Edit .env file with your OAuth credentials"
echo "2. Run: ./start-kolme-zkp.sh"
echo "3. In another terminal, run: ./start-demo-real.sh"
echo "4. Open http://localhost:3000 in your browser"
echo ""
echo "The system is now using real cryptographic proofs!"
echo ""
echo "⚠️  Important: The current trusted setup is for TESTING ONLY."
echo "For production, organize a proper ceremony with multiple participants."