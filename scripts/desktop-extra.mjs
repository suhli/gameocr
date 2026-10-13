// Run after desktop-smoke, against the finished portable binary with GPU visibility disabled.
import { chromium } from '@playwright/test';
import { spawn,execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=process.cwd(),out=path.join(root,'artifacts/validation/recovery');
await fs.mkdir(out,{recursive:true});
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn,timeout=30000){const end=Date.now()+timeout;while(Date.now()<end){try{const v=await fn();if(v)return v;}catch{}await delay(250);}throw Error('Condition timed out');}
const env={...process.env,GAMEOCR_DATA_DIR:path.join(out,'user-data'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9224',CUDA_VISIBLE_DEVICES:'-1'};
for(const key of Object.keys(env))if(key.toLowerCase()==='path'||key.startsWith('CUDA_PATH'))delete env[key];
env.Path=`${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`;
const binary=path.resolve(process.env.GAMEOCR_APP||'artifacts/GameOCR-windows-x64-portable/GameOCR.exe');
const log=await fs.open(path.join(out,'native.log'),'w');
const app=spawn(binary,[],{env,stdio:['ignore',log.fd,log.fd],windowsHide:true});
let browser,invoke,fixture;const checks=[];
const passed=(name,detail={})=>{checks.push({name,status:'passed',...detail});console.log('PASS',name,JSON.stringify(detail));};
try{
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9224'));
  const page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>/tauri.localhost|tauri:\/\//.test(p.url())));
  invoke=(command,args={})=>page.evaluate(([command,args])=>window.__TAURI_INTERNALS__.invoke(command,args),[command,args]);
  await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  const settings=await invoke('get_settings');
  await invoke('update_settings',{settings:{...settings,modelDir:process.env.GAMEOCR_MODEL_DIR||path.join(root,'artifacts/validation/desktop/user-data/models'),maxTokens:256,maxImageEdge:800,device:'auto'}});
  const model=(await invoke('list_models')).find(m=>m.installed&&m.imported);assert(model);
  await invoke('load_model',{id:model.id});const status=await invoke('get_model_status');assert.equal(status.device,'CPU');assert.match(status.message,/CUDA/);passed('No visible GPU: automatic CPU fallback',{message:status.message});
  const adbPath='C:/Program Files/Netease/MuMu Player 12/shell/adb.exe';
  if(await fs.stat(adbPath).catch(()=>false)){const s=await invoke('get_settings');await invoke('update_settings',{settings:{...s,adbPath}});const devices=await invoke('list_adb_devices');passed('Real ADB executable + device enumeration',{devices});if(!devices.some(d=>d.status==='device'))checks.push({name:'ADB PNG capture / reconnect',status:'not-tested',reason:'No connected Android device'});}
  const mumu=(await invoke('list_windows',{showAll:false})).find(w=>/MuMu/i.test(w.processName)&&w.capturable);
  if(mumu){await invoke('select_window',{id:mumu.id});await invoke('start_capture');const preview=await until(()=>invoke('get_capture_preview'));await invoke('save_screenshot',{path:path.join(out,'mumu.png'),recognized:false});passed('MuMu main window WGC',{process:mumu.processName,width:preview.width,height:preview.height});await invoke('recognize_once');const result=(await invoke('get_history'))[0];assert(result.result.rawOutput.length>0);await invoke('export_result',{frameId:result.result.frameId,path:path.join(out,'mumu-ocr.json')});passed('CPU OCR on real MuMu main window',{blocks:result.result.blocks.length,ms:result.result.metrics.totalMs});await invoke('stop_capture');}
  else checks.push({name:'MuMu WGC',status:'not-tested',reason:'No capturable MuMu window'});
  const control=path.join(out,'fixture-control.txt');await fs.rm(control,{force:true});
  fixture=spawn(path.join(root,'target/debug/capture-fixture.exe'),[control],{stdio:['ignore',log.fd,log.fd],windowsHide:false});
  const target=await until(async()=>(await invoke('list_windows',{showAll:false})).find(w=>w.title==='GameOCR Capture Fixture'));
  await invoke('select_window',{id:target.id});await invoke('start_capture');await until(()=>invoke('get_capture_preview'));await invoke('recognize_once');
  const cpu=(await invoke('get_history'))[0];assert.equal(cpu.result.device,'CPU');assert(cpu.result.blocks.some(b=>/Health/i.test(b.text)));
  await invoke('export_result',{frameId:cpu.result.frameId,path:path.join(out,'cpu-result.json')});passed('Real CPU fallback WGC OCR',{ms:cpu.result.metrics.totalMs,blocks:cpu.result.blocks.length});await invoke('stop_capture');
  await invoke('unload_model');
  const bad=path.join(out,'invalid.gguf');await fs.writeFile(bad,Buffer.concat([Buffer.from('GGUF'),Buffer.alloc(124)]));
  const imported=await invoke('import_model',{model:bad,mmproj:bad});
  await assert.rejects(()=>invoke('load_model',{id:imported.id}));assert.equal((await invoke('get_model_status')).status,'error');passed('Invalid model produces real error');
  await invoke('unload_model');await invoke('delete_model',{id:imported.id});await invoke('load_model',{id:model.id});assert.equal((await invoke('get_model_status')).status,'ready');passed('Unload / delete / reload after failed model');
  const workerPid=Number(execFileSync('powershell.exe',['-NoProfile','-Command',`Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq ${app.pid} -and $_.Name -eq 'ocr-worker.exe' } | Select-Object -ExpandProperty ProcessId`],{encoding:'utf8',windowsHide:true}).trim());assert(workerPid>0);
  await invoke('plugin:window|close',{label:'main'}).catch(()=>{});await until(()=>app.exitCode!==null,10000);assert.equal(app.exitCode,0);passed('Exit with resident CPU model');
  const remaining=execFileSync('powershell.exe',['-NoProfile','-Command',`Get-Process -Id ${workerPid} -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id; exit 0`],{encoding:'utf8',windowsHide:true}).trim();assert.equal(remaining,'');
  assert.deepEqual(await fs.readdir(path.join(out,'user-data/sessions')),[]);passed('Exit removes worker and temporary session');
}catch(error){checks.push({name:'failure',status:'failed',message:error.stack});throw error;}
finally{await fs.writeFile(path.join(out,'report.json'),JSON.stringify({checks},null,2));if(invoke&&app.exitCode===null)await Promise.race([invoke('unload_model').catch(()=>{}),delay(5000)]);await browser?.close();app.kill();fixture?.kill();await log.close();}
