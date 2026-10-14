# Windows packaging templates

Based on the official Tauri CLI **2.12.2** templates (the version in package-lock.json):

- https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.2/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
- https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.2/crates/tauri-bundler/src/bundle/windows/msi/main.wxs

Retained under the upstream MIT license, copied alongside these files. Local changes:

- NSIS uses per-file compression instead of `/SOLID`. The combined CUDA/cuDNN
  uncompressed payload exceeds the default solid block's size limit, although
  the compressed installer fits below 2 GiB.
- WiX uses embedded automatically split CABs with a 1024 MiB uncompressed target
  instead of a single CAB. The largest individual file must still fit in a CAB.

All upstream install/upgrade/uninstall/WebView behavior remains in the templates.
When updating the Tauri CLI lockfile, compare these templates with the matching
upstream release and retain only these two changes. Do not edit generated files
under target/ as the production fix.
