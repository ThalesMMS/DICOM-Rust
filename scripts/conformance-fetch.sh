#!/usr/bin/env bash
set -euo pipefail
cache="${1:?usage: conformance-fetch.sh <cache directory>}"
mkdir -p "$cache"
cache="$(cd "$cache" && pwd)"
itu=https://www.itu.int/wftp3/Public/t/testsignal/SpeImage
nema=ftp://medical.nema.org/medical/dicom/DataSets/WG04
jxl_commit=b1d0f990b03e57bf6d137c365cd5dc8b470b9191
jxl_objects=https://storage.googleapis.com/jxl-conformance/objects
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}
fetch() {
  local dir="$cache/$1/archive" file="$2" url="$3" want="$4" got
  mkdir -p "$dir"
  if [ -f "$dir/$file" ] && [ "$(sha256 "$dir/$file")" = "$want" ]; then
    return
  fi
  echo "fetching $url" >&2
  curl --fail --silent --show-error --location --retry 3 --output "$dir/$file.part" "$url"
  got="$(sha256 "$dir/$file.part")"
  if [ "$got" != "$want" ]; then
    rm -f "$dir/$file.part"
    echo "$url: SHA-256 $got, expected $want" >&2
    exit 1
  fi
  mv "$dir/$file.part" "$dir/$file"
}
version=1
complete() { [ "$(cat "$cache/$1/data/.complete" 2>/dev/null)" = "$version" ]; }
begin() { rm -rf "$cache/$1/data" && mkdir -p "$cache/$1/data"; }
finish() { echo "$version" >"$cache/$1/data/.complete"; }
if ! complete itu-t83; then
  fetch itu-t83 T83_data_files.zip "$itu/T83/v1994_11/T83_data_files.zip" \
    9f5ed0a74a3087c6c93f4cbb0bd5128c2a87b128e4f59f5b6649e32941da7069
  begin itu-t83
  unzip -q "$cache/itu-t83/archive/T83_data_files.zip" -d "$cache/itu-t83/data"
  finish itu-t83
fi
if ! complete itu-t87; then
  fetch itu-t87 T87_data_set.zip "$itu/T87/v1998_06/T87_data_set.zip" \
    182a17ee23eea27f2361b3443ddcbdce7c10263c2a417aaf1a190e4efc6a6fc2
  begin itu-t87
  unzip -q "$cache/itu-t87/archive/T87_data_set.zip" -d "$cache/itu-t87/data"
  finish itu-t87
fi
if ! complete itu-t803; then
  fetch itu-t803 T803_codestreams.zip \
    "$itu/T803/v2024_02/T.803v3_15444-4ed4-ElecAtt-codestreams.zip" \
    ac04b52e1fe38404912036c14f215099ea9a785f38644fbe76ae8f3d1523c86d
  begin itu-t803
  unzip -q "$cache/itu-t803/archive/T803_codestreams.zip" -d "$cache/itu-t803/data"
  finish itu-t803
fi
if ! complete nema-wg04; then
  fetch nema-wg04 compsamples_refanddir.tar.bz2 "$nema/compsamples_refanddir.tar.bz2" \
    2477265a9b1435230f0288ae2a30e26de512f6c2b9cd5f6f04bc1a0d5b539c96
  fetch nema-wg04 compsamples_jpeg.tar "$nema/compsamples_jpeg.tar" \
    df4487f691added9ed863424a166db31e954b2acf8efa299b2235259ab5b1883
  fetch nema-wg04 compsamples_jpegls.tar "$nema/compsamples_jpegls.tar" \
    6bb8c17a96693ec15285db717628806e557019d9b21d7fd53aa6cbaf225b0504
  fetch nema-wg04 compsamples_j2k.tar "$nema/compsamples_j2k.tar" \
    fc2e9a00f25c2caab75223ac2dd34b814c07de184c4ca2973a0449019e125ce4
  fetch nema-wg04 compsamples_rle_20040210.tar.bz2 "$nema/compsamples_rle_20040210.tar.bz2" \
    bdd2d3fc2a1777158938705f5c53c7f15c332ae140acd65ecbe16230870b8e29
  begin nema-wg04
  for archive in compsamples_refanddir.tar.bz2 compsamples_jpeg.tar compsamples_jpegls.tar \
    compsamples_j2k.tar; do
    tar -xf "$cache/nema-wg04/archive/$archive" -C "$cache/nema-wg04/data"
  done
  mkdir "$cache/nema-wg04/data/rle"
  tar -xf "$cache/nema-wg04/archive/compsamples_rle_20040210.tar.bz2" \
    -C "$cache/nema-wg04/data/rle"
  finish nema-wg04
fi
if ! complete jxl-conformance; then
  fetch jxl-conformance "conformance-$jxl_commit.tar.gz" \
    "https://codeload.github.com/libjxl/conformance/tar.gz/$jxl_commit" \
    5d006fb1e709e9736499b945044f41a2b9e4d9d9c403469fa849138e9559bef0
  for object in \
    cad070b944d8aff6b7865c211e44bc469b08addf5b4a19d11fdc4ef2f7494d1b \
    1d8471e3b7f0768f408b5e5bf5fee0de49ad6886d846712b1aaa702379722e2b \
    eac8c30907e41e53a73a0c002bc39e998e0ceb021bd523f5bff4580b455579e6 \
    4aaea4e1bda3771e62643fcdf2003ffe6048ee2870c93f67d34d6cc16cb7da4b \
    c0b86989e287649b944f1734ce182d1c1ac0caebf12cec7d487d9793f51f5b8f \
    851abd36b93948cfaeabeb65c8bb8727ebca4bb1d2697bce73461e05ccf24c1e \
    953d3ada476e3218653834c9addc9c16bb6f9f03b18be1be8a85c07a596ea32d; do
    fetch jxl-conformance "$object" "$jxl_objects/$object" "$object"
  done
  begin jxl-conformance
  tar -xzf "$cache/jxl-conformance/archive/conformance-$jxl_commit.tar.gz" \
    -C "$cache/jxl-conformance/data" --strip-components 1
  mkdir "$cache/jxl-conformance/data/objects"
  for object in "$cache/jxl-conformance/archive"/*; do
    case "$(basename "$object")" in
      *.tar.gz) ;;
      *) cp "$object" "$cache/jxl-conformance/data/objects/" ;;
    esac
  done
  finish jxl-conformance
fi
echo "conformance cache ready: $cache"
