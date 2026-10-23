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
- NVIDIA CUDA Runtime and cuBLAS: proprietary NVIDIA licenses. Application-private
  runtime DLLs are distributed under their upstream terms. The applicable toolkit EULA
  is copied to `licenses/CUDA-EULA.txt`. Redistributable components:
  https://docs.nvidia.com/cuda/eula/index.html
- WebView2 uses the existing system runtime in the Windows 11 portable edition.
  It is not included in that archive.
- Model weights are **not distributed** in application artifacts. Official model
  (Apache-2.0): https://huggingface.co/lightonai/LightOnOCR-3-0.8B
  Built-in GGUF catalog: https://huggingface.co/Prathat/LightOnOCR-3-0.8B-GGUF

No third-party game assets are included. Test images are generated locally with
Windows fonts solely for OCR validation.

## Added multi-runtime dependencies

- ort 2.0.0-rc.12: MIT / Apache-2.0 — https://github.com/pykeio/ort
- ONNX Runtime 1.24.3: MIT, with separate third-party notices —
  https://github.com/microsoft/onnxruntime/releases/tag/v1.24.3
  Releases include the original LICENSE and ThirdPartyNotices.txt under licenses/.
  The standard base packages CPU libraries; GPU core/provider libraries are
  downloaded from the pinned Microsoft Release and installed with its original
  LICENSE and ThirdPartyNotices.txt. TensorRT is not included.
- imageproc 0.25.0: MIT — https://github.com/image-rs/imageproc
- CUDA12 GPU distribution also includes application-private cuFFT, NVRTC and
  nvJitLink runtime DLLs needed by the providers. The standard base downloads
  these components from NVIDIA into its private user cache, retaining their
  licenses. Full offline builds store them once in workers/cuda-runtime. Both
  layouts make the libraries visible only to GPU worker processes. The
  exact toolkit EULA is included as licenses/CUDA-EULA.txt; no nvcc or Toolkit
  installer is included. https://docs.nvidia.com/cuda/eula/index.html
- cuDNN 9.8.0.87: NVIDIA proprietary software, not covered by GameOCR's MIT license.
  The official redistributable archive's full license is retained as
  licenses/cuDNN-LICENSE.txt. Its Section 1.2 redistribution conditions apply;
  recipients receive the NVIDIA license with the application, and the DLLs are
  provided as application runtime components, not as a standalone SDK.
  https://docs.nvidia.com/deeplearning/cudnn/backend/v9.8.0/reference/eula.html
- PP-OCRv6 official PaddlePaddle ONNX model cards and PaddleOCR dictionary identify
  Apache-2.0 licensing. Weights and dictionary are downloaded on demand and are
  not bundled in ZIP/MSI/NSIS. Catalog entries preserve their source and license.
  https://huggingface.co/PaddlePaddle/PP-OCRv6_small_det_onnx
  https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec_onnx
  https://huggingface.co/PaddlePaddle/PP-OCRv6_medium_det_onnx
  https://huggingface.co/PaddlePaddle/PP-OCRv6_medium_rec_onnx
  https://github.com/PaddlePaddle/PaddleOCR/blob/main/LICENSE

Build and distribution operators must preserve these notices, the NVIDIA runtime
license texts, and the upstream attribution files when repackaging. The CPU-only
runtime layout can be distributed without NVIDIA DLLs.

The matching GameOCR llama.cpp CUDA Worker is a separate application component,
distributed as a versioned companion ZIP with GameOCR/llama.cpp dependency notices
and the build toolchain's CUDA EULA. Its compiled-in download hash must match the
base application's catalog; it is not a generic upstream llama.cpp executable.
