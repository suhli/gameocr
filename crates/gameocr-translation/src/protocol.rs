use crate::types::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag="type",rename_all="camelCase")]
pub enum WorkerRequest {
    Load { protocol_version:u32, config:TranslationLoad },
    Translate { request:TranslationRequest },
    Unload,
    Shutdown,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag="type",rename_all="camelCase")]
pub enum WorkerResponse {
    Ready { device:String, load_ms:f64 },
    Result { result:GenerationResult },
    Unloaded,
    Error { message:String },
}
#[cfg(feature="inference")]
pub fn serve() -> anyhow::Result<()> {
    use std::io::{self,BufRead,Read,Write};
    let mut engine:Option<Box<dyn crate::engine::TranslationEngine>>=None;
    let mut input=io::stdin().lock();let mut output=io::stdout().lock();
    loop {
        let mut line=Vec::new();let count=(&mut input).take(262145).read_until(b'\n',&mut line)?;
        if count==0 { break; }
        anyhow::ensure!(count<=262144 && line.last()==Some(&b'\n'),"Translation IPC message too large or incomplete");
        let request=serde_json::from_slice::<WorkerRequest>(&line);
        let shutdown=matches!(&request,Ok(WorkerRequest::Shutdown));
        let response:anyhow::Result<WorkerResponse>=(|| match request? {
            WorkerRequest::Load{protocol_version,config}=>{anyhow::ensure!(protocol_version==PROTOCOL_VERSION,"Unsupported translation protocol");engine.take();let start=std::time::Instant::now();let e=crate::engine::TranslationFactory::create(config)?;let device=e.device().to_owned();engine=Some(e);Ok(WorkerResponse::Ready{device,load_ms:start.elapsed().as_secs_f64()*1000.})},
            WorkerRequest::Translate{request}=>Ok(WorkerResponse::Result{result:engine.as_mut().ok_or_else(||anyhow::anyhow!("Translation model not loaded"))?.translate(request)?}),
            WorkerRequest::Unload|WorkerRequest::Shutdown=>{engine.take();Ok(WorkerResponse::Unloaded)}
        })();
        writeln!(output,"{}",serde_json::to_string(&response.unwrap_or_else(|e|WorkerResponse::Error{message:format!("{e:#}")}))?)?;output.flush()?;
        if shutdown {break;}
    }
    Ok(())
}
