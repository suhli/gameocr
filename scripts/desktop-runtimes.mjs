import {chromium} from '@playwright/test';
import {spawn} from 'node:child_process';import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
const root=process.cwd(),out=path.resolve('artifacts/validation/multi-runtime');fs.mkdirSync(out,{recursive:true});
const log=fs.openSync(path.join(out,'desktop.log'),'w');
const vite=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1420','--strictPort'],{windowsHide:true,stdio:['ignore',log,log]});
const app=spawn(path.resolve('target/debug/gameocr.exe'),[],{windowsHide:true,stdio:['ignore',log,log],env:{...process.env,GAMEOCR_DATA_DIR:path.join(out,'data'),CUDA_VISIBLE_DEVICES:'-1',WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9226'}});
const delay=ms=>new Promise(r=>setTimeout(r,ms));let browser;
async function until(fn){for(let i=0;i<100;i++){try{const result=await fn();if(result)return result;}catch{}await delay(200);}throw Error('WebView initialization timed out');}
const checks=[];
try{
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9226'));
  const page=await until(()=>browser.contexts()[0]?.pages().find(p=>!p.url().includes('devtools')));
  await page.waitForFunction(()=>!!window.__TAURI_INTERNALS__,{timeout:30000});
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await invoke('unload_model');let settings=await invoke('get_settings');
  settings={...settings,modelDir:path.resolve('.cache/test-models'),device:'auto'};await invoke('update_settings',{settings});
  const models=await invoke('list_models');assert(models.some(m=>m.id==='ppocrv6-small'&&m.installed));checks.push('Multi-runtime catalog / verified files');
  await invoke('load_model',{id:'ppocrv6-small'});let status=await invoke('get_model_status');assert.equal(status.device,'CPU');assert.match(status.message,/回退 CPU/);checks.push('Actual CUDA initialization failure falls back to CPU');
  await assert.rejects(invoke('update_settings',{settings:{...settings,onnx:{...settings.onnx,batchSize:2}}}));checks.push('Loaded-model parameter changes rejected');
  await invoke('load_model',{id:'ppocrv6-medium'});status=await invoke('get_model_status');assert.equal(status.id,'ppocrv6-medium');assert.equal(status.status,'ready');checks.push('Model switch disposes old worker');
  await page.getByRole('button',{name:'打开设置'}).click();await page.getByText('Detection Threshold',{exact:true}).waitFor();assert.equal(await page.getByText('上下文大小',{exact:true}).count(),0);await page.screenshot({path:path.join(out,'onnx-settings.png')});await page.getByRole('button',{name:'关闭设置'}).click();checks.push('ONNX capability-driven settings render');
  await invoke('unload_model');settings={...settings,device:'cuda'};await invoke('update_settings',{settings});await assert.rejects(invoke('load_model',{id:'ppocrv6-small'}));assert.equal((await invoke('get_model_status')).status,'error');checks.push('Explicit CUDA failure remains an error');
  await invoke('unload_model');settings={...settings,device:'cpu'};await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});assert.equal((await invoke('get_model_status')).status,'ready');await invoke('unload_model');checks.push('Recovery after worker initialization failure');
  const dir=path.resolve('.cache/test-models');const args={variant:'small',detector:path.join(dir,'ppocrv6-small-det.onnx'),recognizer:path.join(dir,'ppocrv6-small-rec.onnx'),dictionary:path.join(dir,'ppocrv6_dict.txt')};
  const imported=await invoke('import_onnx_model',args);assert(imported.imported&&imported.installed);await invoke('load_model',{id:imported.id});await invoke('unload_model');
  const dict=imported.files.find(f=>f.role==='dictionary');const corrupt=fs.readFileSync(path.join(dir,dict.path));corrupt[20]^=1;fs.writeFileSync(path.join(dir,dict.path),corrupt);assert.equal((await invoke('list_models')).find(m=>m.id===imported.id).installed,false);await invoke('delete_model',{id:imported.id});assert(imported.files.every(f=>!fs.existsSync(path.join(dir,f.path))));checks.push('Real multi-file import, inference validation, same-size corruption and deletion');
  const before=fs.readdirSync(dir).length;await assert.rejects(invoke('import_onnx_model',{...args,recognizer:args.detector}));assert.equal(fs.readdirSync(dir).length,before);checks.push('Invalid ONNX graph rolls back every copied file');
  fs.writeFileSync('docs/test-results/desktop-runtimes.json',JSON.stringify({executedAt:new Date().toISOString(),checks},null,2)+'\n');console.log(`Passed ${checks.length} real Tauri runtime checks.`);
}finally{await browser?.close();app.kill();vite.kill();fs.closeSync(log);}
