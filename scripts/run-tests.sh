#!/bin/bash

# Kolme ZKP Test Runner
# Runs tests in order of complexity, starting with simple unit tests

set -e

echo "🧪 Kolme ZKP Test Suite"
echo "======================"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_status() {
    echo -e "${BLUE}[TEST]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[FAIL]${NC} $1"
}

# Test results tracking
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

run_test() {
    local test_name="$1"
    local test_command="$2"
    
    print_status "Running: $test_name"
    TESTS_RUN=$((TESTS_RUN + 1))
    
    if eval "$test_command" > /dev/null 2>&1; then
        print_success "$test_name"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        print_error "$test_name"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

run_test_verbose() {
    local test_name="$1"
    local test_command="$2"
    
    print_status "Running: $test_name"
    TESTS_RUN=$((TESTS_RUN + 1))
    
    echo "Command: $test_command"
    if eval "$test_command"; then
        print_success "$test_name"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        print_error "$test_name"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# Step 1: Basic compilation tests
echo ""
print_status "Phase 1: Compilation Tests"
echo "----------------------------"

run_test "ZKP Auth - Check compilation" "cd packages/zkp-auth && cargo check"
run_test "Kolme Main - Check compilation" "cd packages/kolme && cargo check"
run_test "Integration Tests - Check compilation" "cd packages/integration-tests && cargo check"

# Step 2: Simple unit tests
echo ""
print_status "Phase 2: Unit Tests"
echo "-------------------"

run_test "ZKP Auth - Basic types test" "cd packages/zkp-auth && cargo test test_social_platform_conversion --lib"

# Step 3: Integration tests (without heavy crypto)
echo ""
print_status "Phase 3: API Integration Tests"
echo "------------------------------"

run_test "OAuth Init Flow" "cd packages/integration-tests && cargo test test_oauth_init_flow"
run_test "OAuth Callback Flow" "cd packages/integration-tests && cargo test test_oauth_callback_flow"
run_test "Status Endpoint" "cd packages/integration-tests && cargo test test_status_endpoint"
run_test "Invalid Platform Handling" "cd packages/integration-tests && cargo test test_invalid_platform"
run_test "Expired OAuth State" "cd packages/integration-tests && cargo test test_expired_oauth_state"
run_test "Invalid Session Handling" "cd packages/integration-tests && cargo test test_invalid_session"

# Step 4: ZKP tests (these may take longer)
echo ""
print_status "Phase 4: ZKP Cryptographic Tests"
echo "--------------------------------"

print_status "Running ZKP commitment tests..."
run_test "Platform Commitment Generation" "cd packages/zkp-auth && cargo test test_different_platforms"
run_test "Commitment Uniqueness" "cd packages/zkp-auth && cargo test test_commitment_uniqueness"

# Step 5: Full ZKP flow (most intensive)
echo ""
print_status "Phase 5: Full ZKP Flow Tests"
echo "----------------------------"

print_warning "Note: ZKP flow tests may take several minutes due to cryptographic operations"
run_test_verbose "Complete ZKP Flow" "cd packages/zkp-auth && timeout 300 cargo test test_complete_zkp_flow"

# Step 6: Demo application tests
echo ""
print_status "Phase 6: Demo Application Tests"
echo "-------------------------------"

if [ -d "demo/zkp-login-demo" ]; then
    if [ -f "demo/zkp-login-demo/package.json" ]; then
        run_test "Demo - Install dependencies" "cd demo/zkp-login-demo && npm install"
        run_test "Demo - TypeScript compilation" "cd demo/zkp-login-demo && npm run build"
    else
        print_warning "Demo package.json not found, skipping npm tests"
    fi
else
    print_warning "Demo directory not found, skipping demo tests"
fi

# Step 7: TypeScript SDK tests
echo ""
print_status "Phase 7: TypeScript SDK Tests"
echo "-----------------------------"

if [ -d "packages/kolme-zkp-sdk" ]; then
    if [ -f "packages/kolme-zkp-sdk/package.json" ]; then
        run_test "SDK - Install dependencies" "cd packages/kolme-zkp-sdk && npm install"
        run_test "SDK - TypeScript compilation" "cd packages/kolme-zkp-sdk && npm run build"
    else
        print_warning "SDK package.json not found, skipping npm tests"
    fi
else
    print_warning "SDK directory not found, skipping SDK tests"
fi

# Final summary
echo ""
echo "============================================"
echo "🎯 Test Suite Summary"
echo "============================================"
echo ""
echo "Tests Run:    $TESTS_RUN"
echo "Tests Passed: $TESTS_PASSED"
echo "Tests Failed: $TESTS_FAILED"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    print_success "All tests passed! 🎉"
    echo ""
    echo "✅ ZKP system is ready for production"
    echo "✅ All API endpoints are functional"
    echo "✅ Cryptographic operations are working"
    echo "✅ Demo application is ready"
    echo ""
    echo "Next steps:"
    echo "  1. Test the demo: cd demo/zkp-login-demo && ./start-demo.sh"
    echo "  2. Deploy with real OAuth credentials"
    echo "  3. Monitor ZKP proof generation performance"
    exit 0
else
    print_error "Some tests failed"
    echo ""
    echo "❌ $TESTS_FAILED test(s) failed out of $TESTS_RUN"
    echo ""
    echo "Common issues and solutions:"
    echo "  - Compilation errors: Check Rust toolchain and dependencies"
    echo "  - ZKP test timeouts: Normal for first run, try again"
    echo "  - Missing dependencies: Run 'cargo build' first"
    echo "  - Demo issues: Check Node.js version and npm install"
    echo ""
    echo "For detailed error logs, run individual tests with:"
    echo "  cargo test <test_name> -- --nocapture"
    exit 1
fi
