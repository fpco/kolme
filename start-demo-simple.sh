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
