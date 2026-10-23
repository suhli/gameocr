<script lang="ts">
  import { onMount } from 'svelte';
  import { X,Download,FolderInput,HardDrive,Check,LoaderCircle,Trash2,Power,ArrowDownToLine } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import type { ModelEntry,ModelStatus,DownloadProgress } from '../types';
  import { api } from '../services/api';
  import TranslationModels from './TranslationModels.svelte';
  let modelType=$state('ocr');
  let {models,status,progress,onClose,onRefresh,onError}:{models:ModelEntry[];status:ModelStatus;progress:DownloadProgress|null;onClose:()=>void;onRefresh:()=>Promise<void>;onError:(e:unknown)=>void}=$props();
  let variant=$state('small');let runtimeFilter=$state('all');let dialog:HTMLDialogElement;let busy=$state('');let deleting=$state('');let error=$state('');
  function fail(e:unknown){error=String(e);onError(e);}
  const downloading=$derived(progress&&['downloading','verifying','retrying'].includes(progress.state));
  async function run(id:string,action:()=>Promise<unknown>){busy=id;error='';try{await action();await onRefresh();}catch(e){fail(e);}finally{busy='';}}
  async function importLocal(){const model=await api.pickModel('选择 LightOnOCR-3 主模型 GGUF');if(typeof model!=='string')return;const mmproj=await api.pickModel('选择匹配的 mmproj GGUF');if(typeof mmproj!=='string')return;await run('import',()=>api.importModel(model,mmproj));}
  async function importOnnx(){
    const detector=await api.pickOnnxFile('选择 PP-OCRv6 检测模型','onnx');if(typeof detector!=='string')return;
    const recognizer=await api.pickOnnxFile('选择匹配的 PP-OCRv6 识别模型','onnx');if(typeof recognizer!=='string')return;
    const dictionary=await api.pickOnnxFile('选择完整 PP-OCRv6 字典','txt');if(typeof dictionary!=='string')return;
    await run('import',()=>api.importOnnx(variant,detector,recognizer,dictionary));
  }
  async function remove(model:ModelEntry){deleting=model.id;try{if(await api.confirmDelete(model.name))await run(model.id,()=>api.deleteModel(model.id));}catch(e){onError(e);}finally{deleting='';}}
  const size=(bytes:number)=>`${(bytes/1024/1024/1024).toFixed(2)} GB`;
  onMount(()=>dialog.showModal());
</script>
<dialog bind:this={dialog} oncancel={onClose} class="model-dialog">
  <header class="row spread"><div><div class="label">LOCAL INFERENCE</div><h2><HardDrive/> 模型管理</h2></div><button class="icon-btn" onclick={onClose} aria-label="关闭模型管理"><X/></button></header>
  <nav class="model-types"><button class:active={modelType==='ocr'} onclick={()=>modelType='ocr'}>OCR 模型</button><button class:active={modelType==='translation'} onclick={()=>modelType='translation'}>翻译模型</button></nav>
  {#if modelType==='translation'}<TranslationModels/>{:else}
  <div class="intro"><strong>本地 OCR 模型</strong><p>GGUF 使用 llama.cpp，PP-OCRv6 使用 ONNX Runtime。所有文件完成完整性校验后才可加载。</p><div class="row"><select aria-label="Runtime 筛选" bind:value={runtimeFilter}><option value="all">所有 Runtime</option><option value="llamacpp">llama.cpp / GGUF</option><option value="onnx">ONNX Runtime</option></select><span class="chip">实际设备：{status.device??'未加载'}</span></div>{#if status.message}<p>{status.message}</p>{/if}</div>
  <div class="models">
    {#each models.filter(m=>runtimeFilter==='all'||m.runtime===runtimeFilter) as model}
      <article class:active={status.id===model.id&&status.status==='ready'}>
        <div class="model-main"><div class="model-symbol"><HardDrive/></div><div><div class="row"><strong>{model.name}</strong>{#if model.id==='Q8_0'}<span class="recommended">推荐</span>{/if}{#if model.installed}<span class="installed"><Check/> 已校验</span>{/if}</div><p>{model.runtime} · {model.adapter} · {size(model.files.reduce((sum,f)=>sum+f.size,0))}</p><p>{model.capabilities.nativeCoordinates?'原生四点坐标':model.capabilities.grounding?'Grounding 坐标':'纯文字'}{model.capabilities.confidence?' · 置信度':''} · {model.files.length} 个文件</p></div></div>
        <div class="row actions">
          {#if busy===model.id}<LoaderCircle class="spinner"/>{/if}
          {#if model.installed}
            {#if status.id===model.id&&['ready','error'].includes(status.status)}<Button variant="secondary" size="sm" disabled={!!busy} onclick={()=>run(model.id,api.unloadModel)}><Power/> 卸载</Button>
            {:else}<Button variant="secondary" size="sm" disabled={!!busy||['loading','unloading'].includes(status.status)} onclick={()=>run(model.id,()=>api.loadModel(model.id))}>加载模型</Button>{/if}
            <button class="icon-btn" aria-label={`删除 ${model.quantization}`} disabled={!!busy||!!deleting||status.status!=='unloaded'} onclick={()=>remove(model)}><Trash2/></button>
          {:else}<Button variant="outline" size="sm" disabled={!!busy||!!downloading||model.imported} onclick={()=>run(model.id,()=>api.downloadModel(model.id))}><Download/> {model.imported?'文件损坏，请重新导入':'下载'}</Button>
            {#if model.imported}<button class="icon-btn" aria-label={`删除 ${model.name}`} disabled={!!busy||!!deleting||status.status!=='unloaded'} onclick={()=>remove(model)}><Trash2/></button>{/if}
          {/if}
        </div>
      </article>
    {/each}
  </div>
  {#if error}<pre class="dialog-error" role="alert">{error}</pre>{/if}
  {#if progress&&progress.state!=='complete'}<div class="download"><div class="row spread"><span><ArrowDownToLine/> {progress.id}</span><span>{progress.state==='failed'?'下载失败，点击下载可续传':progress.state==='verifying'?'正在校验 SHA-256':progress.state==='retrying'?`正在重试 (${progress.attempt}/3)`:'正在下载'}</span></div>{#if progress.total}<progress value={progress.received??0} max={progress.total}></progress><small>{((progress.received??0)/1024/1024).toFixed(0)} / {(progress.total/1024/1024).toFixed(0)} MB · 支持断点续传</small>{/if}</div>{/if}
  <footer><div class="row spread"><Button variant="outline" disabled={!!busy||!!downloading} onclick={()=>importLocal().catch(onError)}><FolderInput/> 导入 GGUF</Button><div class="row"><select aria-label="ONNX 模型规格" bind:value={variant}><option value="small">PP-OCRv6 small</option><option value="medium">PP-OCRv6 medium</option></select><Button variant="outline" disabled={!!busy||!!downloading} onclick={()=>importOnnx().catch(onError)}><FolderInput/> 导入 ONNX</Button></div></div><p class="muted">GGUF：Prathat 社区转换 · ONNX：PaddlePaddle 官方 · ONNX 导入时运行 CPU 模型校验</p></footer>
  {/if}
</dialog>
<style>
  .model-types{display:flex;gap:24px;padding:0 26px;border-bottom:1px solid var(--border)}.model-types button{padding:14px 0;background:none;color:var(--muted);border:0;border-bottom:2px solid transparent;font-size:12px}.model-types button.active{color:var(--primary);border-color:var(--primary)}
  dialog{width:690px;max-width:90vw;padding:0;border:1px solid var(--border);border-radius:12px;background:var(--panel);color:var(--foreground);box-shadow:0 24px 100px #0009}dialog::backdrop{background:#0009;backdrop-filter:blur(4px)}header{padding:23px 26px;border-bottom:1px solid var(--border)}h2{font-size:19px;margin:8px 0 0;display:flex;align-items:center;gap:10px;font-weight:600}.intro{padding:24px 26px 18px}.intro p{color:var(--muted);font-size:12px;line-height:1.8;margin:10px 0 14px}.models{padding:0 26px;max-height:45vh;overflow:auto}article{display:flex;align-items:center;justify-content:space-between;padding:18px 0;border-top:1px solid var(--border)}.model-main{display:flex;align-items:center;gap:13px;min-width:0}.model-symbol{display:grid;place-items:center;width:38px;height:38px;border:1px solid var(--border);border-radius:8px;background:var(--subtle);color:var(--muted)}.model-main strong{font-family:Consolas,monospace;font-size:14px}.model-main p{font-size:11px;color:var(--muted);margin:7px 0 0;max-width:310px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.active .model-symbol{color:var(--primary);border-color:var(--primary)}.recommended{font-size:9px;padding:2px 5px;background:#3b76602c;color:var(--primary);border-radius:3px}.installed{color:var(--muted);display:flex;align-items:center;gap:3px;font-size:10px}.installed :global(svg){width:11px;height:11px}.actions{flex-shrink:0}footer{padding:23px 26px;border-top:1px solid var(--border);font-size:10px;margin-top:8px}.download{margin:15px 26px;padding:15px;border:1px solid var(--border);border-radius:6px;font-size:11px}.download span:first-child{display:flex;align-items:center;gap:5px}progress{width:100%;height:5px;accent-color:var(--primary);margin:12px 0}
</style>
