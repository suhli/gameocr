import { invoke,isTauri } from '@tauri-apps/api/core';
import { listen,type UnlistenFn } from '@tauri-apps/api/event';
import { open,save,confirm } from '@tauri-apps/plugin-dialog';
import type { WindowInfo,Preview,ModelEntry,ModelStatus,Settings,Performance,Snapshot,CaptureStatus,OcrStatus,AdbDevice,DownloadProgress } from '../types';
export const native=isTauri();
function call<T>(command:string,args?:Record<string,unknown>):Promise<T>{if(!native)return Promise.reject(new Error('请通过 GameOCR 桌面应用运行，浏览器预览无法访问本机捕获和模型。'));return invoke<T>(command,args);}
export const api={
  windows:(showAll=false)=>call<WindowInfo[]>('list_windows',{showAll}),selectWindow:(id:string)=>call<WindowInfo>('select_window',{id}),
  adbDevices:()=>call<AdbDevice[]>('list_adb_devices'),connectAdb:(address:string)=>call<string>('connect_adb',{address}),
  startCapture:()=>call<void>('start_capture'),pauseCapture:()=>call<void>('pause_capture'),resumeCapture:()=>call<void>('resume_capture'),stopCapture:()=>call<void>('stop_capture'),preview:()=>call<Preview|null>('get_capture_preview'),
  startOcr:()=>call<void>('start_ocr'),stopOcr:()=>call<void>('stop_ocr'),recognizeOnce:()=>call<void>('recognize_once'),
  models:()=>call<ModelEntry[]>('list_models'),downloadModel:(id:string)=>call<void>('download_model',{id}),importModel:(model:string,mmproj:string)=>call<ModelEntry>('import_model',{model,mmproj}),
  loadModel:(id:string)=>call<void>('load_model',{id}),unloadModel:()=>call<void>('unload_model'),deleteModel:(id:string)=>call<void>('delete_model',{id}),
  settings:()=>call<Settings>('get_settings'),updateSettings:(settings:Settings)=>call<void>('update_settings',{settings}),performance:()=>call<Performance>('get_performance'),modelStatus:()=>call<ModelStatus>('get_model_status'),history:()=>call<Snapshot[]>('get_history'),
  status:()=>call<{capture:CaptureStatus;ocr:OcrStatus;selected:WindowInfo|null;warning:string|null}>('get_status'),
  exportResult:(frameId:number,path:string)=>call<void>('export_result',{frameId,path}),saveScreenshot:(path:string,recognized:boolean)=>call<void>('save_screenshot',{path,recognized}),
  originalScreenshot:(recognized:boolean)=>call<string>('get_original_screenshot',{recognized}),
  pickModel:(title:string)=>open({title,multiple:false,filters:[{name:'GGUF 模型',extensions:['gguf']}]}),
  importOnnx:(variant:string,detector:string,recognizer:string,dictionary:string)=>call<ModelEntry>('import_onnx_model',{variant,detector,recognizer,dictionary}),
  pickOnnxFile:(title:string,extension:string)=>open({title,multiple:false,filters:[{name:extension.toUpperCase(),extensions:[extension]}]}),
  pickDirectory:()=>open({title:'选择模型存储目录',directory:true,multiple:false}),
  pickAdb:()=>open({title:'选择 adb.exe',multiple:false,filters:[{name:'ADB',extensions:['exe']}]}),
  saveJson:()=>save({defaultPath:'gameocr-result.json',filters:[{name:'JSON',extensions:['json']}]}),
  savePng:()=>save({defaultPath:'gameocr-frame.png',filters:[{name:'PNG',extensions:['png']}]}),
  confirmDelete:(name:string)=>confirm(`删除本地模型 ${name}？之后可以重新下载。`,{title:'删除模型',kind:'warning'}),
};
export interface Events{'capture-status':CaptureStatus;'capture-preview':Preview;'capture-error':string;'ocr-result':Snapshot;'ocr-status':OcrStatus;'ocr-error':string;'model-status':ModelStatus;'model-download-progress':DownloadProgress;'performance-update':Performance;}
export function subscribe<K extends keyof Events>(event:K,handler:(value:Events[K])=>void):Promise<UnlistenFn>{return listen<Events[K]>(event,e=>handler(e.payload));}
/** Handles teardown even when async listener registration resolves after unmount. */
export class Subscriptions{private stopped=false;private off:UnlistenFn[]=[];async add<K extends keyof Events>(event:K,handler:(value:Events[K])=>void){const off=await subscribe(event,handler);if(this.stopped)off();else this.off.push(off);}dispose(){this.stopped=true;this.off.splice(0).forEach(fn=>fn());}}
