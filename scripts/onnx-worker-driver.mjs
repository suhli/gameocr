// Direct native adapter driver. Measurements include worker image decode and
// inference, but exclude desktop capture/PNG encoding and preview rendering.
import fs from 'node:fs';import path from 'node:path';import {spawn} from 'node:child_process';import readline from 'node:readline';
export async function startNative(engine){
  const executable=path.resolve(process.env.GAMEOCR_ONNX_WORKER??`src-tauri/resources/workers/onnx/${engine.device}/onnx-worker.exe`);
  const env={...process.env,ORT_DYLIB_PATH:path.join(path.dirname(executable),'onnxruntime.dll')};
  if(engine.device==='cuda')env.Path=path.resolve('src-tauri/resources/workers/cuda-runtime')+';'+(env.Path??env.PATH??'');
  const child=spawn(executable,[],{cwd:path.dirname(executable),windowsHide:true,stdio:['pipe','pipe','pipe'],env});
  let pending,stderr='',ended=false;let frameId=0;
  const exited=new Promise(resolve=>child.once('close',code=>{ended=true;if(pending){clearTimeout(pending.timer);pending.reject(Error(`Worker exited ${code}: ${stderr}`));pending=null;}resolve(code);}));
  child.stderr.on('data',d=>stderr=(stderr+d).slice(-12000));
  child.on('error',e=>{if(pending){clearTimeout(pending.timer);pending.reject(e);pending=null;}});
  readline.createInterface({input:child.stdout}).on('line',line=>{if(!pending)return;const p=pending;pending=null;clearTimeout(p.timer);try{p.resolve(JSON.parse(line));}catch(e){p.reject(e);}});
  function request(value){if(pending||ended)throw Error('Worker unavailable/busy');return new Promise((resolve,reject)=>{pending={resolve,reject,timer:setTimeout(()=>{pending=null;child.kill();reject(Error('Native OCR timeout: '+stderr));},240000)};child.stdin.write(JSON.stringify(value)+'\n');});}
  async function close(){if(!ended){try{await request({type:'shutdown'});}catch{}child.stdin.end();}await exited;}
  const ready=await request({type:'loadEngine',protocol_version:2,config:engine});
  return {ready,request,close,recognize: image=>request({type:'recognize',image:path.resolve(image),frame_id:++frameId,timestamp:Date.now(),mode:'grounding',cache_context:[1,1,0]}),executable};
}
