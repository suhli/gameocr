use anyhow::{Result,Context,bail};
use gameocr_core::types::Device;
use gameocr_translation::{types::*,protocol::{WorkerRequest,WorkerResponse}};
use std::{path::Path,sync::{Arc,atomic::AtomicBool}};
pub struct TranslationWorker {inner:crate::ocr::Worker,pub device:String,pub load_ms:f64}
impl TranslationWorker {
    pub async fn load(root:&Path,config:TranslationLoad,cancel:Arc<AtomicBool>)->Result<Self> {
        let mut inner=if config.device==Device::Cuda {
            let root=root.to_owned();let launch=tokio::task::spawn_blocking(move||crate::cuda_runtime::launch(&root,"translation")).await??;
            crate::ocr::Worker::spawn_configured(&launch.executable,launch.libraries,None,cancel).await?
        }else{crate::ocr::Worker::spawn(&root.join("workers/translation/cpu/translation-worker.exe"),cancel).await?};
        let response:WorkerResponse=inner.request_typed(WorkerRequest::Load{protocol_version:PROTOCOL_VERSION,config}).await?;
        match response {WorkerResponse::Ready{device,load_ms}=>Ok(Self{inner,device,load_ms}),_=>bail!("翻译 Worker 未就绪")}
    }
    pub async fn translate(&mut self,request:TranslationRequest)->Result<GenerationResult> {
        let id=request.request_id.clone();let response:WorkerResponse=self.inner.request_typed(WorkerRequest::Translate{request}).await?;
        match response {WorkerResponse::Result{result} if result.request_id==id=>Ok(result),_=>bail!("翻译 Worker 请求身份不匹配")}
    }
    pub async fn shutdown(&mut self){self.inner.shutdown().await;}
    pub fn memory_mb(&self)->Option<f64> {
        use windows::Win32::System::{Threading::{OpenProcess,PROCESS_QUERY_INFORMATION,PROCESS_VM_READ},ProcessStatus::{GetProcessMemoryInfo,PROCESS_MEMORY_COUNTERS}};
        unsafe {let h=OpenProcess(PROCESS_QUERY_INFORMATION|PROCESS_VM_READ,false,self.inner.pid()?).ok()?;
            let mut counters=PROCESS_MEMORY_COUNTERS::default();counters.cb=std::mem::size_of_val(&counters) as u32;
            let ok=GetProcessMemoryInfo(h,&mut counters,counters.cb).is_ok();let _=windows::Win32::Foundation::CloseHandle(h);
            ok.then_some(counters.WorkingSetSize as f64/1048576.)}
    }
}
pub async fn free_vram()->Result<f64> {
    let output=tokio::time::timeout(std::time::Duration::from_secs(3),tokio::process::Command::new("nvidia-smi")
        .args(["--id=0","--query-gpu=memory.free","--format=csv,noheader,nounits"]).creation_flags(0x08000000).kill_on_drop(true).output()).await??;
    anyhow::ensure!(output.status.success(),"NVIDIA 驱动不可用");
    String::from_utf8(output.stdout)?.trim().parse().context("无法读取可用显存")
}
