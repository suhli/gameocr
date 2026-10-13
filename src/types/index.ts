export type CaptureStatus='idle'|'starting'|'running'|'paused'|'stopping'|'error';
export type OcrStatus='idle'|'waiting'|'processing'|'error';
export interface WindowInfo {id:string;hwnd:string;pid:number;processName:string;title:string;executablePath?:string;width:number;height:number;visible:boolean;minimized:boolean;capturable:boolean;icon?:string;}
export interface OcrBlock{id:string;text:string;label?:string;bbox?:[number,number,number,number];}
export interface OcrResult{frameId:number;timestamp:number;sourceWidth:number;sourceHeight:number;blocks:OcrBlock[];rawOutput:string;metrics:{preprocessingMs:number;inferenceMs:number;parsingMs:number;totalMs:number};device:string;truncated:boolean;}
export interface Preview{frameId:number;generation:number;width:number;height:number;dataUrl:string;}
export interface Snapshot{result:OcrResult;preview:Preview;}
export interface ModelEntry{id:string;name:string;quantization:string;modelFile:string;mmprojFile:string;modelSize:number;mmprojSize:number;modelSha256:string;mmprojSha256:string;installed:boolean;imported:boolean;}
export interface ModelStatus{status:'unloaded'|'loading'|'ready'|'unloading'|'error';id:string|null;device:string|null;loadMs:number|null;message:string|null;}
export interface Settings{captureFps:number;ocrIntervalMs:number;changeDetection:boolean;changeThreshold:number;mode:'plain'|'grounding';device:'auto'|'cpu'|'cuda';threads:number;contextSize:number;maxTokens:number;temperature:number;maxImageEdge:number;modelDir:string;adbPath:string;adbSerial:string;captureSource:'window'|'adb';theme:'dark'|'light';recentWindow:string|null;historyLimit:number;downloadMirror:string;}
export interface Performance{captureFps:number;ocrPerMinute:number;lastOcrMs:number|null;averageOcrMs:number|null;preprocessingMs:number|null;inferenceMs:number|null;skippedFrames:number;droppedFrames:number;queue:string;modelLoadMs:number|null;device:string|null;vramMb:number|null;}
export interface AdbDevice{serial:string;status:string;description:string;}
export interface DownloadProgress{id:string;file?:string;state:string;received?:number;total?:number;attempt?:number;}
