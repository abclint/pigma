#!/bin/sh
# Build a static libasound for the aarch64-unknown-linux-musl cross target.
# rodio/alsa-sys links against libasound, and a musl binary cannot link a
# glibc-provided shared library, so we compile alsa-lib ourselves and install
# it into the musl sysroot that `cross` uses.
set -eux

SYSROOT="/usr/local/aarch64-linux-musl"
ALSA_VERSION="1.2.13"
ALSA_URL="https://github.com/alsa-project/alsa-lib/releases/download/v${ALSA_VERSION}/alsa-lib-${ALSA_VERSION}.tar.bz2"

# Host tools needed to fetch and build alsa-lib.
apt-get update
apt-get install --assume-yes --no-install-recommends wget make

cd /tmp
wget -q "$ALSA_URL"
tar xf "alsa-lib-${ALSA_VERSION}.tar.bz2"
cd "alsa-lib-${ALSA_VERSION}"

export CC="aarch64-linux-musl-gcc"
export CFLAGS="-fPIC"

./configure \
    --host=aarch64-linux-musl \
    --prefix="$SYSROOT" \
    --enable-static \
    --disable-shared \
    --disable-python \
    --without-versioned

make -j"$(nproc)"
make install

# Make sure pkg-config (run by alsa-sys) finds our static lib.
ls "$SYSROOT/lib/pkgconfig/alsa.pc"
