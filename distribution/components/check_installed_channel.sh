#!/usr/bin/env bash
# Exercise the Linux binary produced by service tests. Only loopback is available to the tested process.
set -euo pipefail
binary=$(realpath "$1")
test -x "$binary"
script=$(realpath "$(dirname "$0")/test_installed_channel.py")
offline_check=$(realpath "$(dirname "$0")/check_offline_onnx.sh")
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
cd "$work"
python3 -m venv venv
venv/bin/pip install --quiet 'tuf==7.0.1' 'securesystemslib[crypto]==1.6.0'
curl --fail --location --retry 2 -o runtime.tgz \
  https://github.com/timoncool/onnxruntime/releases/download/v1.32.0/onnxruntime-linux-x64-1.32.0.tgz
curl --fail --location --retry 2 -o crisper-config.json \
  https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/config.json
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj '/CN=Temporary channel QA root' \
  -addext 'basicConstraints=critical,CA:TRUE' -keyout ca.key -out ca.pem
openssl req -new -newkey rsa:2048 -nodes -subj '/CN=localhost' -keyout tls.key -out tls.csr
printf '%s\n' 'subjectAltName=DNS:localhost' 'extendedKeyUsage=serverAuth' 'basicConstraints=CA:FALSE' > tls.ext
openssl x509 -req -days 1 -in tls.csr -CA ca.pem -CAkey ca.key -CAcreateserial -extfile tls.ext -out tls.pem
sudo unshare --net bash -c 'ip link set lo up; exec setpriv --reuid="$1" --regid="$2" --clear-groups env SSL_CERT_FILE="$PWD/ca.pem" "$PWD/venv/bin/python" "$3" "$4" runtime.tgz tls.pem tls.key' \
  bash "$(id -u)" "$(id -g)" "$script" "$binary"
sudo unshare --net bash -c 'ip link set lo up; exec setpriv --reuid="$1" --regid="$2" --clear-groups bash "$3" "$4" runtime.tgz' \
  bash "$(id -u)" "$(id -g)" "$offline_check" "$binary"
