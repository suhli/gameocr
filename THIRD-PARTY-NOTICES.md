# Third-party components

GameOCR source code is MIT licensed. Dependencies retain their own licenses.
`scripts/collect-licenses.mjs` collects installed Rust and production JavaScript
package metadata and license texts into `licenses/dependencies.txt` in releases.

- Tauri: MIT / Apache-2.0 — https://github.com/tauri-apps/tauri
- Svelte, Tailwind CSS, shadcn-svelte: MIT; Lucide: ISC.
- llama-cpp-2 and vendored llama.cpp / ggml: MIT — https://github.com/utilityai/llama-cpp-rs
- windows-capture: MIT — https://github.com/NiiightmareXD/windows-capture
- Microsoft Visual C++ runtime DLLs: Microsoft's redistribution terms apply;
  app-local CRT files are collected from the build toolchain.
  https://learn.microsoft.com/visualstudio/releases/2022/redistribution
- NVIDIA CUDA Runtime and cuBLAS: proprietary NVIDIA licenses. Only application-private
  cudart, cublas, and cublasLt runtime DLLs are distributed. The applicable toolkit EULA
  is copied to `workers/cuda/CUDA-EULA.txt` when present. Redistributable components:
  https://docs.nvidia.com/cuda/eula/index.html
- WebView2 uses the existing system runtime in the Windows 11 portable edition.
  It is not included in that archive.
- Model weights are **not distributed** in application artifacts. Official model
  (Apache-2.0): https://huggingface.co/lightonai/LightOnOCR-3-0.8B
  Built-in GGUF catalog: https://huggingface.co/Prathat/LightOnOCR-3-0.8B-GGUF

No third-party game assets are included. Test images are generated locally with
Windows fonts solely for OCR validation.
