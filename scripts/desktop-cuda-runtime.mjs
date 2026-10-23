// Actual official download + isolated desktop lifecycle checks. No model or timing mocks.
import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import {createServer} from 'node:net';
import {createHash} from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';

const sourceBinary=path.resolve(process.env.GAMEOCR_APP??'artifacts/GameOCR-windows-x64-ondemand-portable/GameOCR.exe');
const out=path.resolve('artifacts/validation/gpu-backends');fs.mkdirSync(out,{recursive:true});
const stamp=Date.now(),appRoot=path.join(out,'app-'+stamp);
fs.cpSync(path.dirname(sourceBinary),appRoot,{recursive:true});
const binary=path.join(appRoot,'GameOCR.exe');
const data=path.join(out,'data-'+stamp),cache=path.join(data,'runtimes/nvidia'),backends=path.join(data,'runtimes/backends');
const checks=[],observed=[],seeded=[],modelDir=path.resolve('.cache/test-models');
let app,browser,page,invoke,fixture,log;
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn,timeout=60000){const deadline=Date.now()+timeout;while(Date.now()<deadline){const result=await fn();if(result)return result;await delay(100);}throw Error('Condition timed out');}
async function launch(offline=false){
  const port=await new Promise((resolve,reject)=>{const s=createServer();s.once('error',reject);s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolve(p));});});
  log=fs.openSync(path.join(out,offline?'offline.log':'online.log'),'w');
  const env={...process.env,GAMEOCR_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:path.join(data,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port='+port};
  for(const key of Object.keys(env))if(key.toLowerCase()==='path'||key.startsWith('CUDA_PATH')||key==='ORT_DYLIB_PATH')delete env[key];
  env.Path=`${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`;
  if(offline){env.HTTPS_PROXY='http://127.0.0.1:9';env.HTTP_PROXY=env.HTTPS_PROXY;env.ALL_PROXY=env.HTTPS_PROXY;env.NO_PROXY='127.0.0.1,localhost';}
  app=spawn(binary,[],{windowsHide:true,stdio:['ignore',log,log],env});
  browser=await until(async()=>{try{return await chromium.connectOverCDP('http://127.0.0.1:'+port);}catch{return null;}});
  page=await until(()=>browser.contexts()[0]?.pages().find(p=>/tauri.localhost|tauri:\/\//.test(p.url())));
  await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
}
async function close(){
  if(invoke)await invoke('plugin:window|close',{label:'main'}).catch(()=>{});
  if(app)await until(()=>app.exitCode!==null);
  await browser?.close();browser=null;page=null;invoke=null;
  if(log!=null){fs.closeSync(log);log=null;}
}
function parts(){return [cache,backends].flatMap(dir=>fs.existsSync(dir)?fs.readdirSync(dir).filter(n=>n.endsWith('.part')).map(name=>({name,directory:path.basename(dir),bytes:fs.statSync(path.join(dir,name)).size})):[]);}
function modules(worker='onnx-worker.exe'){
  const code=`$children=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=${app.pid} AND Name='${worker}'"); if($children.Count -ne 1){throw 'Expected one owned CUDA worker'}; ConvertTo-Json -Compress -InputObject @((Get-Process -Id $children[0].ProcessId).Modules | Where-Object { $_.ModuleName -match '^(cudart|cublas|cudnn|cufft|nvrtc|nvJitLink|onnxruntime|ocr-worker)' } | Select-Object ModuleName,FileName)`;
  return JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',code],{windowsHide:true,encoding:'utf8'}));
}
try{
  assert(!fs.existsSync(path.join(path.dirname(binary),'workers/cuda-runtime')),'Must use the on-demand package');
  assert(!fs.existsSync(path.join(appRoot,'workers/llamacpp/cuda')));
  assert.deepEqual(fs.readdirSync(path.join(appRoot,'workers/onnx/cuda')),['onnx-worker.exe']);
  await launch();assert.equal(path.resolve((await invoke('get_settings')).modelDir),path.join(data,'models'));
  assert(!fs.existsSync(path.join(data,'runtimes')));assert.equal((await invoke('get_cuda_runtime_status')).state,'idle');
  let settings={...await invoke('get_settings'),modelDir,device:'auto',executionMode:'single',enableParallelInference:false,changeDetection:false};
  await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});
  assert.equal((await invoke('get_model_status')).device,'CPU');assert(!fs.existsSync(path.join(data,'runtimes')));
  checks.push('First launch and Auto load use CPU without starting a runtime download or creating its cache');
  await invoke('unload_model');settings={...settings,device:'cuda'};await invoke('update_settings',{settings});
  await page.reload();await page.getByLabel('选择模型',{exact:true}).selectOption('ppocrv6-small');
  await page.getByRole('button',{name:'加载模型',exact:true}).click();
  await until(async()=>{const p=await invoke('get_cuda_runtime_status');assert.notEqual(p.state,'failed',p.message);return p.state==='downloading'&&p.received>1_000_000;},120000);
  await page.getByLabel('CUDA 运行库下载',{exact:true}).waitFor();await page.screenshot({path:path.join(out,'download-progress.png')});
  await page.getByRole('button',{name:'取消下载',exact:true}).click();
  await until(async()=>(await invoke('get_model_status')).status==='error');
  assert.equal((await invoke('get_cuda_runtime_status')).state,'cancelled');const interrupted=parts();assert(interrupted.some(p=>p.bytes>0));
  checks.push('Explicit CUDA starts the official download; visible progress cancellation preserves an incomplete .part and publishes no model');
  await invoke('unload_model');await invoke('update_settings',{settings:{...settings,device:'cpu'}});await invoke('load_model',{id:'ppocrv6-small'});assert.equal((await invoke('get_model_status')).device,'CPU');await invoke('unload_model');
  checks.push('Cancelling runtime preparation leaves CPU model loading available');
  // Optional reuse of components from a previous real official download. Never seed
  // the interrupted component: the desktop must actually resume its partial file.
  if(process.env.GAMEOCR_CUDA_SEED){
    const seed=path.resolve(process.env.GAMEOCR_CUDA_SEED);
    for(const name of fs.readdirSync(seed)){
      const receipt=path.join(seed,name,'installed.json');if(!fs.existsSync(receipt)||fs.existsSync(path.join(cache,name)))continue;
      const metadata=JSON.parse(fs.readFileSync(receipt,'utf8'));
      if(interrupted.some(p=>p.name.startsWith(metadata.archive_sha256)))continue;
      fs.cpSync(path.join(seed,name),path.join(cache,name),{recursive:true});seeded.push(name);
    }
  }
  await invoke('update_settings',{settings});
  const loading=invoke('load_model',{id:'ppocrv6-small'}).then(()=>null,e=>String(e));
  await until(async()=>{
    const p=await invoke('get_cuda_runtime_status');const previous=observed.at(-1);
    if(!previous||previous.state!==p.state||previous.component!==p.component){observed.push({...p});console.log(p.state,p.component);}
    assert.notEqual(p.state,'failed',p.message);
    const model=await invoke('get_model_status');assert.notEqual(model.status,'error',model.message);
    return model.status==='ready';
  },30*60*1000);
  assert.equal(await loading,null);assert.equal((await invoke('get_model_status')).device,'CUDA');assert.equal(parts().length,0);
  const receipts=fs.readdirSync(cache).filter(n=>fs.existsSync(path.join(cache,n,'installed.json')));assert.equal(receipts.length,6);
  const runtimeModules=modules();assert(runtimeModules.some(m=>m.ModuleName.toLowerCase().startsWith('cudnn')));assert(runtimeModules.some(m=>m.ModuleName.toLowerCase().startsWith('cublas')));
  assert(runtimeModules.some(m=>m.ModuleName.toLowerCase()==='onnxruntime_providers_cuda.dll'));
  assert(runtimeModules.every(m=>path.resolve(m.FileName).toLowerCase().startsWith(path.join(data,'runtimes').toLowerCase()+path.sep)));
  checks.push('Retry completes real Microsoft ONNX GPU archive download; GPU core/providers and NVIDIA components load from validated user caches');
  const control=path.join(out,'fixture-control.txt');fs.rmSync(control,{force:true});
  fixture=spawn(path.resolve('target/debug/capture-fixture.exe'),[control],{windowsHide:true,stdio:'ignore'});
  const target=await until(async()=>(await invoke('list_windows',{showAll:false})).find(w=>w.title==='GameOCR Capture Fixture'));
  await invoke('select_window',{id:target.id});await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  const results=[];
  for(const id of ['ppocrv6-small','ppocrv6-medium']){
    if(id!=='ppocrv6-small')await invoke('load_model',{id});
    await invoke('recognize_once');const result=(await invoke('get_history'))[0].result;assert(result.blocks.some(b=>b.text.includes('Health')));assert.equal(result.device,'CUDA');results.push(result);
  }
  checks.push('Small and Medium perform real CUDA OCR on a WGC frame using the downloaded CUDA 12.8/cuDNN 9.8 libraries');
  await invoke('unload_model');
  const companion=JSON.parse(fs.readFileSync('artifacts/gpu-backends.json','utf8')).packages[0];
  const filename=new URL(companion.url).pathname.split('/').at(-1),offlineDir=path.join(appRoot,'runtime-downloads');
  fs.mkdirSync(offlineDir);fs.copyFileSync(path.resolve('artifacts',filename),path.join(offlineDir,filename));
  const ggufSettings={...settings,modelDir:path.join(data,'gguf-models'),threads:4,maxTokens:2048,maxImageEdge:1280};
  await invoke('update_settings',{settings:ggufSettings});
  const imported=await invoke('import_model',{model:path.resolve('models/LightOnOCR-3-0.8B-Q8_0.gguf'),mmproj:path.resolve('models/LightOnOCR-3-0.8B-mmproj-f16.gguf')});
  assert(imported.installed);await invoke('load_model',{id:imported.id});
  assert.equal((await invoke('get_model_status')).device,'CUDA');await invoke('recognize_once');
  const gguf=(await invoke('get_history'))[0].result;assert(gguf.blocks.some(b=>b.text.includes('Health')));results.push(gguf);
  const ggufModules=modules('ocr-worker.exe');assert(ggufModules.some(m=>m.ModuleName.toLowerCase()==='ocr-worker.exe'));
  assert(ggufModules.every(m=>path.resolve(m.FileName).toLowerCase().startsWith(path.join(data,'runtimes').toLowerCase()+path.sep)));
  const ggufBackend=path.dirname(ggufModules.find(m=>m.ModuleName.toLowerCase()==='ocr-worker.exe').FileName);
  const backendReceipt=JSON.parse(fs.readFileSync(path.join(ggufBackend,'installed.json'),'utf8'));
  assert.equal(backendReceipt.files.filter(f=>f.name.startsWith('license-')).length,4,'Retain GameOCR, dependencies, CUDA EULA and third-party notices');
  await invoke('stop_capture');await invoke('unload_model');fixture.kill();fixture=null;
  fs.rmSync(path.join(offlineDir,filename));
  checks.push('Unpublished GameOCR GPU companion archive installed through the hash-verified offline path; cached llama.cpp GPU Worker performs real WGC OCR (not an online Release download test)');
  await invoke('update_settings',{settings});
  await close();await launch(true);
  settings=await invoke('get_settings');await invoke('update_settings',{settings:{...settings,device:'auto'}});
  await invoke('load_model',{id:'ppocrv6-small'});assert.equal((await invoke('get_model_status')).device,'CUDA');assert.equal((await invoke('get_cuda_runtime_status')).state,'idle');await invoke('unload_model');
  await invoke('update_settings',{settings:{...settings,device:'cuda'}});await invoke('load_model',{id:'ppocrv6-small'});assert.equal((await invoke('get_model_status')).device,'CUDA');assert.equal((await invoke('get_cuda_runtime_status')).total,0);await invoke('unload_model');
  checks.push('Restart with unreachable HTTP/HTTPS proxy: Auto and explicit CUDA both load from validated local cache without downloading');
  await invoke('update_settings',{settings:ggufSettings});await invoke('load_model',{id:imported.id});assert.equal((await invoke('get_model_status')).device,'CUDA');assert.equal((await invoke('get_cuda_runtime_status')).total,0);await invoke('unload_model');
  checks.push('Cached llama.cpp CUDA backend also loads offline after removing the companion archive');
  await close();
  fs.writeFileSync('docs/test-results/gpu-backends-desktop.json',JSON.stringify({executedAt:new Date().toISOString(),app:path.relative(process.cwd(),sourceBinary),appSha256:createHash('sha256').update(fs.readFileSync(binary)).digest('hex'),checks,interrupted,seededFromPreviousOfficialDownload:seeded,observed,receipts,runtimeModules,ggufModules,backendReceipt,companion,results},null,2)+'\n');
  console.log('Passed '+checks.length+' official runtime download / real desktop checks');
}catch(e){await page?.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});throw e;}
finally{await invoke?.('cancel_cuda_runtime_download').catch(()=>{});await invoke?.('stop_capture').catch(()=>{});await invoke?.('unload_model').catch(()=>{});await browser?.close();fixture?.kill();app?.kill();if(log!=null)fs.closeSync(log);}
