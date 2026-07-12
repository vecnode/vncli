#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# check_internet.sh
# Network diagnostics for vncli settings menu.
#
# Performs multi-signal internet checks and prints compact I/O counters.
# ---------------------------------------------------------------------------


ADAPTER_STATE="DOWN"
RX_BYTES="NA"
TX_BYTES="NA"
PING_OK=0
DNS_OK=0
INTERNET_STATUS="OFF"

RX_SUM=0
TX_SUM=0
HAS_ADAPTER=0

for iface_path in /sys/class/net/*; do
  iface="$(basename "$iface_path")"
  [[ "$iface" == "lo" ]] && continue

  state_file="$iface_path/operstate"
  rx_file="$iface_path/statistics/rx_bytes"
  tx_file="$iface_path/statistics/tx_bytes"

  if [[ -r "$state_file" ]]; then
    state="$(cat "$state_file")"
    if [[ "$state" == "up" || "$state" == "unknown" ]]; then
      ADAPTER_STATE="UP"
      HAS_ADAPTER=1
    fi
  fi

  if [[ -r "$rx_file" ]]; then
    rx_value="$(cat "$rx_file" 2>/dev/null || echo 0)"
    [[ "$rx_value" =~ ^[0-9]+$ ]] || rx_value=0
    RX_SUM=$((RX_SUM + rx_value))
  fi

  if [[ -r "$tx_file" ]]; then
    tx_value="$(cat "$tx_file" 2>/dev/null || echo 0)"
    [[ "$tx_value" =~ ^[0-9]+$ ]] || tx_value=0
    TX_SUM=$((TX_SUM + tx_value))
  fi
done

if [[ "$HAS_ADAPTER" -eq 1 ]]; then
  RX_BYTES="$RX_SUM"
  TX_BYTES="$TX_SUM"
fi

if ping -c 1 -W 2 1.1.1.1 >/dev/null 2>&1; then
  PING_OK=1
fi

if getent hosts www.microsoft.com >/dev/null 2>&1; then
  DNS_OK=1
elif command -v nslookup >/dev/null 2>&1 && nslookup www.microsoft.com >/dev/null 2>&1; then
  DNS_OK=1
fi

if [[ "$PING_OK" -eq 1 && "$DNS_OK" -eq 1 ]]; then
  INTERNET_STATUS="ON"
fi

echo ""
if [[ "$INTERNET_STATUS" == "ON" ]]; then
  echo "[OK] Internet status: ON"
else
  echo "[ERROR] Internet status: OFF"
fi

if [[ "$ADAPTER_STATE" == "UP" ]]; then
  echo "[INFO] Network adapter state: at least one adapter is UP"
else
  echo "[WARNING] Network adapter state: no active adapter detected"
fi

if [[ "$PING_OK" -eq 1 ]]; then
  echo "[INFO] Reachability test (ICMP 1.1.1.1): PASS"
else
  echo "[INFO] Reachability test (ICMP 1.1.1.1): FAIL"
fi

if [[ "$DNS_OK" -eq 1 ]]; then
  echo "[INFO] DNS test (www.microsoft.com): PASS"
else
  echo "[INFO] DNS test (www.microsoft.com): FAIL"
fi

echo ""
echo "[INFO] Small I/O summary (all adapters combined):"
if [[ "$RX_BYTES" == "NA" || "$TX_BYTES" == "NA" ]]; then
  echo "[WARNING] Unable to read RX/TX byte counters."
else
  echo "[INFO] RX bytes: $RX_BYTES"
  echo "[INFO] TX bytes: $TX_BYTES"
fi

echo ""
exit 0