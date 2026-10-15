export type CaptureStatus='idle'|'starting'|'running'|'paused'|'stopping'|'error';
export type OcrStatus='idle'|'waiting'|'processing'|'error';
export interface WindowInfo {id:string;hwnd:string;pid:number;processName:string;title:string;executablePath?:string;width:number;height:number;visible:boolean;minimized:boolean;capturable:boolean;icon?:string;}
export interface OcrBlock{id:string;text:string;label?:string;bbox?:[number,number,number,number];confidence?:number|null;polygon?:{x:number;y:number}[]|null;}
export interface OcrResult{frameId:number;timestamp:number;sourceWidth:number;sourceHeight:number;blocks:OcrBlock[];rawOutput:string;metrics:{preprocessingMs:number;inferenceMs:number;parsingMs:number;totalMs:number;detectionMs?:number;recognitionMs?:number;execution?:ExecutionMetrics};device:string;truncated:boolean;runtime?:string;modelId?:string;engineId?:string;}
export interface Preview{frameId:number;generation:number;width:number;height:number;dataUrl:string;}
export interface Snapshot{result:OcrResult;preview:Preview;}
export interface OcrCapabilities{plain:boolean;grounding:boolean;nativeCoordinates:boolean;confidence:boolean;generation:boolean;}
export interface ModelFile{role:string;path:string;size:number;sha256:string;url?:string|null;}
export interface ModelEntry{id:string;name:string;schemaVersion:number;runtime:string;adapter:string;version:string;quantization:string;capabilities:OcrCapabilities;files:ModelFile[];source:string;parameters:Record<string,unknown>|null;installed:boolean;imported:boolean;}
export interface OnnxConfig{detectionThreshold:number;boxThreshold:number;unclipRatio:number;recognitionThreshold:number;batchSize:number;detectionResize:number;}
export interface ModelStatus{status:'unloaded'|'loading'|'ready'|'unloading'|'error';id:string|null;device:string|null;loadMs:number|null;message:string|null;}
export interface ParallelConfig{enableParallelInference:boolean;enableCpuWorkers:boolean;gpuWorkerCount:number;cpuWorkerCount:number;}
export interface Settings extends ParallelConfig{captureFps:number;ocrIntervalMs:number;changeDetection:boolean;changeThreshold:number;mode:'plain'|'grounding';device:'auto'|'cpu'|'cuda';threads:number;contextSize:number;maxTokens:number;temperature:number;maxImageEdge:number;onnx:OnnxConfig;modelDir:string;adbPath:string;adbSerial:string;captureSource:'window'|'adb';theme:'dark'|'light';recentWindow:string|null;historyLimit:number;downloadMirror:string;}
export interface Performance{captureFps:number;ocrPerMinute:number;lastOcrMs:number|null;averageOcrMs:number|null;preprocessingMs:number|null;inferenceMs:number|null;skippedFrames:number;droppedFrames:number;queue:string;modelLoadMs:number|null;device:string|null;vramMb:number|null;execution?:ExecutorStatus|null;}
export interface AdbDevice{serial:string;status:string;description:string;}
export interface DownloadProgress{id:string;file?:string;state:string;received?:number;total?:number;attempt?:number;}

export interface ExecutionMetrics{batchId:number;endToEndMs:number;transferMs:number;splitMs:number;queueWaitMs:number;workerWallMs:number;mergeMs:number;cpuTasks:number;gpuTasks:number;failedTasks:number;retriedTasks:number;tileCount:number;wholeFrameFallback:boolean;speedup:number|null;}
export interface WorkerStatus{id:number;pid:number|null;device:string;state:string;threads:number;completedTasks:number;failedTasks:number;averageMs:number;}
export interface ExecutorStatus{parallel:boolean;queuedTasks:number;frameId:number|null;workers:WorkerStatus[];last:ExecutionMetrics|null;}
