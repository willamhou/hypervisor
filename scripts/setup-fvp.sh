#!/bin/bash
# Setup Arm FVP (Fixed Virtual Platform) for S-EL2 SPMC testing
#
# Prerequisites:
# 1. Download FVP_Base_RevC-2xAEMvA from:
#    https://developer.arm.com/Tools%20and%20Software/Fixed%20Virtual%20Platforms/Arm%20Architecture%20FVPs
#    (Free, requires Arm account registration)
#
# 2. Place the downloaded .tgz in this directory, then run:
#    ./scripts/setup-fvp.sh
#
# The FVP is more accurate than QEMU for:
# - S-EL2 permission boundaries
# - GICv3/v4 behavior
# - TrustZone NS bit handling
# - Stage-2 translation faults
# - SPMD ↔ SPMC handshake timing

set -euo pipefail

FVP_DIR="${HOME}/fvp"
FVP_TARBALL=$(ls FVP_Base_RevC-2xAEMvA_*.tgz 2>/dev/null | head -1)

# Step 1: Install FVP
if [ -z "${FVP_TARBALL:-}" ]; then
    echo "Error: No FVP_Base_RevC-2xAEMvA_*.tgz found in current directory."
    echo ""
    echo "Download from:"
    echo "  https://developer.arm.com/Tools%20and%20Software/Fixed%20Virtual%20Platforms/Arm%20Architecture%20FVPs"
    echo ""
    echo "Steps:"
    echo "  1. Register for a free Arm account"
    echo "  2. Download 'FVP_Base_RevC-2xAEMvA' for Linux (x86_64)"
    echo "  3. Place the .tgz file in $(pwd)"
    echo "  4. Re-run this script"
    exit 1
fi

echo "[1/4] Installing FVP to ${FVP_DIR}..."
mkdir -p "${FVP_DIR}"
tar xzf "${FVP_TARBALL}" -C /tmp/fvp-extract 2>/dev/null || true
mkdir -p /tmp/fvp-extract
tar xzf "${FVP_TARBALL}" -C /tmp/fvp-extract

# Run installer
INSTALLER=$(find /tmp/fvp-extract -name "FVP_Base_RevC-2xAEMvA.sh" -o -name "*.sh" | head -1)
if [ -n "${INSTALLER}" ]; then
    bash "${INSTALLER}" --i-agree-to-the-contained-eula --no-interactive --destination "${FVP_DIR}"
else
    # Some versions extract directly
    cp -r /tmp/fvp-extract/* "${FVP_DIR}/"
fi
rm -rf /tmp/fvp-extract

# Find the binary
FVP_BIN=$(find "${FVP_DIR}" -name "FVP_Base_RevC-2xAEMvA" -type f | head -1)
if [ -z "${FVP_BIN}" ]; then
    echo "Error: FVP binary not found after installation"
    exit 1
fi

echo "  FVP installed: ${FVP_BIN}"

# Step 2: Verify it runs
echo "[2/4] Verifying FVP..."
"${FVP_BIN}" --version 2>/dev/null || "${FVP_BIN}" --help 2>/dev/null | head -3
echo "  FVP verified."

# Step 3: Create run script for our SPMC
echo "[3/4] Creating run scripts..."

cat > scripts/run-fvp-spmc.sh << 'RUNSCRIPT'
#!/bin/bash
# Run our SPMC on Arm FVP with TF-A
#
# Usage: ./scripts/run-fvp-spmc.sh
#
# Requires: build-tfa-spmc first (builds TF-A + our SPMC as BL32)

set -euo pipefail

FVP_BIN="${HOME}/fvp/bin/FVP_Base_RevC-2xAEMvA"
if [ ! -x "${FVP_BIN}" ]; then
    FVP_BIN=$(find "${HOME}/fvp" -name "FVP_Base_RevC-2xAEMvA" -type f 2>/dev/null | head -1)
fi

if [ -z "${FVP_BIN:-}" ] || [ ! -x "${FVP_BIN}" ]; then
    echo "Error: FVP binary not found. Run scripts/setup-fvp.sh first."
    exit 1
fi

# Check TF-A artifacts exist
if [ ! -f tfa/bl1.bin ] || [ ! -f tfa/fip.bin ]; then
    echo "Error: TF-A artifacts not found. Run 'make build-tfa-spmc' first."
    exit 1
fi

echo "Starting FVP with our SPMC (S-EL2) + BL33..."
echo "Press Ctrl+C to stop"
echo ""

"${FVP_BIN}" \
    -C pctl.startup=0.0.0.0 \
    -C bp.secure_memory=1 \
    -C cluster0.NUM_CORES=4 \
    -C cluster0.has_arm_v8-4=1 \
    -C cluster0.has_amu=1 \
    -C bp.pl011_uart0.untimed_fifos=1 \
    -C bp.pl011_uart1.untimed_fifos=1 \
    -C bp.secureflashloader.fname=tfa/bl1.bin \
    -C bp.flashloader0.fname=tfa/fip.bin \
    -C bp.ve_sysregs.exit_on_shutdown=1 \
    -C cluster0.gicv3.SRE-EL2-enable-RAO=1 \
    -C cluster0.gicv3.cpuintf-mmap-access-level=2 \
    "$@"
RUNSCRIPT

chmod +x scripts/run-fvp-spmc.sh

cat > scripts/run-fvp-unit-tests.sh << 'RUNSCRIPT'
#!/bin/bash
# Run unit tests on Arm FVP (no TF-A, direct kernel load)
#
# Usage: ./scripts/run-fvp-unit-tests.sh

set -euo pipefail

FVP_BIN=$(find "${HOME}/fvp" -name "FVP_Base_RevC-2xAEMvA" -type f 2>/dev/null | head -1)

if [ -z "${FVP_BIN:-}" ]; then
    echo "Error: FVP not found. Run scripts/setup-fvp.sh first."
    exit 1
fi

# Build if needed
if [ ! -f target/aarch64-unknown-none/debug/hypervisor.bin ]; then
    echo "Building hypervisor..."
    make build
fi

echo "Starting FVP with unit tests..."
echo "Press Ctrl+C to stop"
echo ""

timeout 120 "${FVP_BIN}" \
    -C pctl.startup=0.0.0.0 \
    -C bp.secure_memory=0 \
    -C cluster0.NUM_CORES=4 \
    -C cluster0.has_arm_v8-4=1 \
    -C bp.pl011_uart0.untimed_fifos=1 \
    -C bp.ve_sysregs.exit_on_shutdown=1 \
    -C cluster0.gicv3.SRE-EL2-enable-RAO=1 \
    -C cluster0.gicv3.cpuintf-mmap-access-level=2 \
    --application cluster0.cpu0=target/aarch64-unknown-none/debug/hypervisor \
    "$@" 2>&1 || true
RUNSCRIPT

chmod +x scripts/run-fvp-unit-tests.sh

echo "  Created: scripts/run-fvp-spmc.sh"
echo "  Created: scripts/run-fvp-unit-tests.sh"

# Step 4: Summary
echo "[4/4] Done!"
echo ""
echo "Next steps:"
echo "  1. Run unit tests:  ./scripts/run-fvp-unit-tests.sh"
echo "  2. Run SPMC:        make build-tfa-spmc && ./scripts/run-fvp-spmc.sh"
echo ""
echo "FVP vs QEMU differences:"
echo "  - More accurate GICv3 timing and priority handling"
echo "  - Proper S-EL2 permission fault behavior"
echo "  - NS bit enforcement on memory accesses"
echo "  - SPMD handshake timing matches real hardware"
echo ""
echo "FVP binary: ${FVP_BIN}"
