<script lang="ts">
  import { onMount } from 'svelte';
  import { X,Settings2,FolderOpen } from '@lucide/svelte';
  import { Button } from '$lib/components/ui/button';
  import type { Settings,ModelEntry,LearningSummary } from '../types';
  import { api } from '../services/api';
  let {settings,model,onClose,onSave,onError}:{settings:Settings;model?:ModelEntry;onClose:()=>void;onSave:(value:Settings)=>Promise<void>;onError:(e:unknown)=>void}=$props();
  // A settings dialog edits a snapshot; external changes must not overwrite unsaved edits.
  // svelte-ignore state_referenced_locally
  let draft=$state<Settings>(structuredClone($state.snapshot(settings)));let saving=$state(false);let error=$state('');let dialog:HTMLDialogElement;
  onMount(()=>dialog.showModal());
  let learningStats=$state<LearningSummary|null>(null);let learningBusy=$state(false);
  async function refreshLearning(clear=false){learningBusy=true;try{if(clear)await api.clearAdaptiveProfiles();learningStats=await api.adaptiveStatistics();}catch(e){error=String(e);onError(e);}finally{learningBusy=false;}}
  async function save(){saving=true;error='';try{await onSave(draft);onClose();}catch(e){error=String(e);onError(e);}finally{saving=false;}}
</script>
<dialog bind:this={dialog} oncancel={onClose}>
  <header class="row spread"><h2><Settings2/> 设置</h2><button class="icon-btn" aria-label="关闭设置" onclick={onClose}><X/></button></header>
  <div class="body">
    <section><h3>捕获与调度</h3><p>捕获持续更新最新帧，OCR 按独立间隔读取。忙碌时自动丢弃旧帧。</p>
      <div class="fields"><label>截图频率 (FPS)<input type="number" min="1" max="60" bind:value={draft.captureFps}/></label><label>OCR 间隔 (ms)<input type="number" min="100" max="60000" step="100" bind:value={draft.ocrIntervalMs}/></label></div>
      <div class="fields"><label>变化检测阈值<input type="number" min="0" max="1" step="0.01" bind:value={draft.changeThreshold}/></label><label>历史记录上限<input type="number" min="1" max="100" bind:value={draft.historyLimit}/></label></div>
      <label class="check"><input type="checkbox" bind:checked={draft.changeDetection}/> 跳过没有显著变化的画面</label>
    </section>
    <section><h3>推理参数</h3><p>更改这些参数前，请先卸载模型。设置适用于当前选择的模型。</p>
      <div class="fields"><label>推理设备<select bind:value={draft.device}><option value="auto">自动 · CUDA 优先</option><option value="cuda">NVIDIA CUDA</option><option value="cpu">CPU</option></select></label><label>CPU 线程数<input type="number" min="1" max="128" bind:value={draft.threads}/></label></div>
      {#if model?.capabilities.generation??true}
      <div class="fields"><label>上下文大小<input type="number" min="2048" max="32768" step="1024" bind:value={draft.contextSize}/></label><label>最大输出 Token<input type="number" min="1" max="16000" bind:value={draft.maxTokens}/></label></div>
      <div class="fields"><label>采样温度<input type="number" min="0" max="2" step="0.1" bind:value={draft.temperature}/></label><label>图像最长边 (px)<input type="number" min="224" max="2048" step="32" bind:value={draft.maxImageEdge}/></label></div>
      {:else if model?.runtime==='onnx'}
      <div class="fields"><label>Detection Threshold<input type="number" min="0" max="1" step="0.01" bind:value={draft.onnx.detectionThreshold}/></label><label>Box Threshold<input type="number" min="0" max="1" step="0.01" bind:value={draft.onnx.boxThreshold}/></label></div>
      <div class="fields"><label>Unclip Ratio<input type="number" min="0.1" max="5" step="0.1" bind:value={draft.onnx.unclipRatio}/></label><label>识别置信度阈值<input type="number" min="0" max="1" step="0.01" bind:value={draft.onnx.recognitionThreshold}/></label></div>
      <div class="fields"><label>识别 Batch Size<input type="number" min="1" max="32" bind:value={draft.onnx.batchSize}/></label><label>检测最长边 (px)<input type="number" min="32" max="2048" step="32" bind:value={draft.onnx.detectionResize}/></label></div>
      <label class="check"><input type="checkbox" bind:checked={draft.onnx.preserveSmallText}/> 保留高分辨率小字（重叠分块检测）</label>
      <label class="check"><input type="checkbox" disabled={draft.onnx.preserveSmallText} bind:checked={draft.onnx.selectiveDetection}/> 对疑似漏字区域补做高分辨率检测</label>
      {#if draft.onnx.selectiveDetection && !draft.onnx.preserveSmallText}<label>每帧最多补检区域<input type="number" min="1" max="8" bind:value={draft.onnx.refinementRegions}/></label><p>根据原图细节和已有检测框选择区域，不以识别置信度判断是否漏字。预算耗尽后仍可能漏检；密集小字可使用完整分块模式。补检使用单 Worker。</p>{/if}
      <label class="check"><input type="checkbox" bind:checked={draft.onnx.incremental}/> 增量识别：复用像素完全未变的区域与文字</label>
      <label class="check"><input type="checkbox" bind:checked={draft.onnx.widthBuckets}/> 按文字宽度分批，限制每批像素量</label>
      {#if draft.onnx.widthBuckets}<label>每批像素预算（不含颜色通道）<input type="number" min="153600" max="786432" step="1536" bind:value={draft.onnx.recognitionPixelBudget}/></label><p>相近宽度的文字同批识别；超长文本独立分批。Batch Size 仍是每批行数上限。实际速度取决于设备和文字长度分布。</p>{/if}
      {#if draft.onnx.incremental}<p>连续识别时生效。单 Worker 保留一帧缓存，每 10 秒或 30 次识别刷新；切换来源、尺寸或模型后重新识别。此模式替代缩略图变化检测，避免漏掉小字更新。</p>{/if}
      <label class="check"><input type="checkbox" checked={draft.onnx.detectionRegion!=null} onchange={(e)=>draft.onnx.detectionRegion=e.currentTarget.checked?[0,0,1,1]:null}/> 只识别指定区域</label>
      {#if draft.onnx.detectionRegion}
        <p>以完整截图为基准，输入 0–1 的比例。区域以外不识别，结果坐标仍对应完整截图。</p>
        <div class="fields"><label>左边比例<input type="number" min="0" max="0.99" step="0.01" bind:value={draft.onnx.detectionRegion[0]}/></label><label>顶部比例<input type="number" min="0" max="0.99" step="0.01" bind:value={draft.onnx.detectionRegion[1]}/></label><label>宽度比例<input type="number" min="0.01" max="1" step="0.01" bind:value={draft.onnx.detectionRegion[2]}/></label><label>高度比例<input type="number" min="0.01" max="1" step="0.01" bind:value={draft.onnx.detectionRegion[3]}/></label></div>
      {/if}
      {#if draft.onnx.preserveSmallText || draft.onnx.detectionRegion || draft.onnx.incremental}
        <p>局部／小字检测使用单 Worker，先合并检测框，再从原图识别。小字模式优先减少漏字，会增加检测次数；限定区域可减少处理量。检测最长边至少 256 px，最多 64 个局部分块。</p>
      {/if}
      <label>OCR 执行策略<select aria-label="OCR 执行策略" bind:value={draft.executionMode}><option value="single">单 Worker</option><option value="manual">手动并行</option><option value="adaptive">自适应优化</option></select></label>
      <p>单 Worker 保持原有设备选择和完整识别；手动并行使用指定实例。自适应根据传统视觉特征与本机历史选择 CPU 策略，速度以实际端到端耗时为准。</p>
      {#if draft.executionMode==='manual'}
        {#if draft.device==='auto'}<label class="check"><input type="checkbox" bind:checked={draft.enableCpuWorkers}/> CPU 参与混合并行</label>{/if}
        <div class="fields">
          <label>GPU Worker 数量<input type="number" min={draft.device==='cuda'?1:0} max="2" step="1" disabled={draft.device==='cpu'} bind:value={draft.gpuWorkerCount}/></label>
          <label>CPU Worker 数量<input type="number" min={draft.device==='cpu'?1:0} max="4" step="1" disabled={draft.device==='cuda'||(draft.device==='auto'&&!draft.enableCpuWorkers)} bind:value={draft.cpuWorkerCount}/></label>
        </div>
        <p>{draft.device==='cpu'?'CPU 模式只启动 CPU Worker，无需启用混合开关。':draft.device==='cuda'?'CUDA 模式只启动 GPU Worker，加载失败会报错。':'Auto 按以上配置混合调度；GPU 不可用时仍保留 CPU 回退。'} GPU 最多 2 个实例；CPU 最多为逻辑核心数与 4 中的较小值。CPU 线程预算会在多个实例之间分摊；更多 Worker 不保证更快。</p>
      {:else if draft.executionMode==='adaptive'}
        <label class="check"><input type="checkbox" bind:checked={draft.adaptive.learning}/> 启用在线学习</label>
        <label class="check"><input type="checkbox" disabled={!draft.adaptive.learning} bind:checked={draft.adaptive.exploration}/> 允许低频策略探索</label>
        <div class="fields"><label>最大 CPU Worker 数<input type="number" min="1" max="2" step="1" bind:value={draft.adaptive.maxCpuWorkers}/></label><label>最大 GPU Worker 数<input type="number" min="0" max="0" disabled bind:value={draft.adaptive.maxGpuWorkers}/></label></div>
        <p>初期仅选择 CPU×1 或 CPU×2，保留设置的总线程预算。Auto 在自适应模式下仅使用 CPU；显式 CUDA 需要切换为其他策略。GPU 自适应尚未开放。探索最多每 20 次合适机会一次，并有冷却与负载限制；切换模型实例会增加本帧延迟。关闭学习后只读取历史。</p>
        <div class="row"><Button disabled={learningBusy} onclick={()=>refreshLearning()}>查看学习统计</Button><Button disabled={learningBusy} onclick={()=>refreshLearning(true)}>清除已学习的性能记录</Button></div>
        {#if learningStats}<div class="learning-stats" aria-label="自适应学习统计">
          <p>工作负载类别 {learningStats.classes} · CPU×1 {learningStats.selections['cpu-single']??0} 次 · CPU×2 {learningStats.selections['cpu-parallel-2']??0} 次</p>
          <p>成功探索 {learningStats.successfulExplorations} / {learningStats.explorations} · 切换 {learningStats.switches} · 整图复验 {learningStats.fallbacks} · 失败 {learningStats.failedFrames}</p>
          <p>最近端到端均值 {learningStats.recentLatencyMs==null?'—':`${learningStats.recentLatencyMs.toFixed(0)} ms`}。仅保存有上限的特征摘要和性能统计，不保存截图或识别文字。</p>
          {#if learningStats.recovered}<p>历史文件无效，已恢复安全默认值。</p>{/if}
          {#if learningStats.persistenceError}<p role="alert">{learningStats.persistenceError}</p>{/if}
        </div>{/if}
      {/if}
      {/if}
    </section>
    <section><h3>存储与 ADB</h3><label>模型目录<div class="row"><input bind:value={draft.modelDir}/><button class="icon-btn" aria-label="选择模型目录" onclick={async()=>{try{const path=await api.pickDirectory();if(typeof path==='string')draft.modelDir=path;}catch(e){onError(e);}}}><FolderOpen/></button></div></label>
      <label>模型下载源<select bind:value={draft.downloadMirror}><option value="https://huggingface.co">Hugging Face</option><option value="https://hf-mirror.com">HF-Mirror 镜像（同一 SHA-256 校验）</option></select></label>
      <label>ADB 可执行文件<div class="row"><input bind:value={draft.adbPath} placeholder="adb 或完整 adb.exe 路径"/><button class="icon-btn" aria-label="选择 ADB" onclick={async()=>{try{const path=await api.pickAdb();if(typeof path==='string')draft.adbPath=path;}catch(e){onError(e);}}}><FolderOpen/></button></div></label>
    </section>
  </div>
  {#if error}<pre class="dialog-error" role="alert">{error}</pre>{/if}
  <footer class="row spread"><small>修改捕获频率或来源前请停止捕获。</small><Button disabled={saving} onclick={save}>{saving?'保存中…':'保存设置'}</Button></footer>
</dialog>
<style>
  dialog{width:600px;max-height:90vh;padding:0;border:1px solid var(--border);border-radius:12px;background:var(--panel);color:var(--foreground);box-shadow:0 24px 100px #0009}dialog::backdrop{background:#0009;backdrop-filter:blur(4px)}header,footer{padding:20px 26px;border-bottom:1px solid var(--border)}footer{border-top:1px solid var(--border);border-bottom:0}h2{display:flex;align-items:center;gap:8px;font-size:18px;margin:0}.body{padding:0 26px;max-height:67vh;overflow:auto}section{padding:20px 0;border-bottom:1px solid var(--border)}h3{margin:0 0 10px;font-size:13px;font-weight:600}p{color:var(--muted);font-size:11px;line-height:1.7}label{display:block;font-size:11px;color:var(--muted);margin:12px 0}label input,label select{margin-top:7px;font-size:12px}label.check{display:flex;align-items:center;gap:8px}.check input{margin:0}.fields{display:grid;grid-template-columns:1fr 1fr;gap:18px}.fields label{margin:9px 0}.row input{flex:1}
</style>
