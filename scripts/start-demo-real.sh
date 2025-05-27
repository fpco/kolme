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
