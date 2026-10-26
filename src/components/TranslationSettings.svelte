<script lang="ts">
  import {onMount} from 'svelte';import {X} from '@lucide/svelte';import {Button} from '$lib/components/ui/button';
  import {api} from '../services/api';import {translationLanguages,type TranslationSettings,type GameProfile} from '../types/translation';
  import TranslationModels from './TranslationModels.svelte';
  let {onClose,processName=''}:{onClose:()=>void;processName?:string}=$props();
  let dialog:HTMLDialogElement;let settings=$state<TranslationSettings|null>(null),profileJson=$state('[]'),error=$state(''),notice=$state(''),busy=$state(false),tab=$state('settings');
  async function run(fn:()=>Promise<unknown>){busy=true;error='';notice='';try{await fn();}catch(e){error=String(e);}finally{busy=false;}}
  async function save(){if(!settings)return;const profiles:GameProfile[]=JSON.parse(profileJson);if(!Array.isArray(profiles))throw new Error('Profile JSON 必须是数组');
    const previous=settings.profiles;settings.profiles=profiles.map(p=>({...p,version:Math.max(p.version||0,(previous.find(old=>old.id===p.id)?.version??0)+1)}));
    await api.updateTranslationSettings($state.snapshot(settings));settings=await api.translationSettings();profileJson=JSON.stringify(settings.profiles,null,2);notice='翻译设置已保存';}
  function chooseProfile(){if(!settings)return;const p=settings.profiles.find(p=>p.id===settings!.profileId);if(p){settings.sourceLanguage=p.sourceLanguage||'auto';settings.targetLanguage=p.targetLanguage||'zh';settings.style=p.style;}}
  function addProfile(){const p:GameProfile={id:`game-${Date.now()}`,name:'新游戏',sourceLanguage:'auto',targetLanguage:'zh',style:'',background:'',glossary:[],version:1,windowProcess:processName};const all=JSON.parse(profileJson);all.push(p);profileJson=JSON.stringify(all,null,2);}
  async function importProfiles(){const path=await api.pickOnnxFile('导入游戏 Profile JSON','json');if(typeof path==='string')profileJson=JSON.stringify(await api.importTranslationProfiles(path),null,2);}
  async function exportProfiles(){await save();const path=await api.saveJson();if(path)await api.exportTranslationProfiles(path);}
  onMount(()=>{dialog.showModal();void run(async()=>{settings=await api.translationSettings();profileJson=JSON.stringify(settings.profiles,null,2);});});
</script>
<dialog bind:this={dialog} oncancel={onClose}>
  <header class="row spread"><h2>本地翻译</h2><button class="icon-btn" aria-label="关闭翻译设置" onclick={onClose}><X/></button></header>
  <nav>{#each [['settings','翻译设置'],['models','翻译模型'],['profiles','游戏与术语']] as [id,label]}<button class:active={tab===id} onclick={()=>tab=id}>{label}</button>{/each}</nav>
  {#if tab==='models'}<TranslationModels/>{:else if settings}
  <div class="body">
    {#if tab==='settings'}
      <label class="check"><input type="checkbox" bind:checked={settings.enabled}/> OCR 完成后自动翻译</label>
      <div class="grid">
        <label>源语言<select bind:value={settings.sourceLanguage}><option value="auto">Auto · 自动判断</option>{#each translationLanguages as [id,name]}<option value={id}>{name}</option>{/each}</select></label>
        <label>目标语言<select bind:value={settings.targetLanguage}>{#each translationLanguages as [id,name]}<option value={id}>{name}</option>{/each}</select></label>
        <label>翻译设备<select bind:value={settings.device}><option value="auto">Auto · 允许回退 CPU</option><option value="cpu">CPU</option><option value="cuda">CUDA · 失败时提示</option></select></label>
        <label>资源策略<select bind:value={settings.resourcePolicy}><option value="balanced">Balanced · 保留游戏显存</option><option value="fast">Fast · 较少显存预留</option><option value="cpuOnly">CPU Only</option><option value="manual">Manual</option></select></label>
        <label>对白稳定性<select bind:value={settings.stability} onchange={()=>{if(settings)settings.debounceMs=({fast:80,balanced:350,accurate:700} as Record<string,number>)[settings.stability];}}><option value="fast">Fast</option><option value="balanced">Balanced</option><option value="accurate">Accurate</option></select></label>
        <label>稳定等待（ms）<input type="number" min="0" max="3000" bind:value={settings.debounceMs}/></label>
        <label>文字分组<select bind:value={settings.grouping}><option value="auto">自动 · 保守合并相邻对白</option><option value="block">逐块</option><option value="paragraph">整段</option></select></label>
        <label>游戏 Profile<select bind:value={settings.profileId} onchange={chooseProfile}><option value={null}>不使用</option>{#each settings.profiles as p}<option value={p.id}>{p.name}{p.windowProcess?` · ${p.windowProcess}`:''}</option>{/each}</select></label>
      </div>
      <label>翻译风格<input bind:value={settings.style} placeholder="可选，例如保留角色语气"/></label>
      <label class="check"><input type="checkbox" bind:checked={settings.batchTranslation}/> 合批翻译独立短文本（条目不匹配时逐组重试）</label>
      <p>OCR 设备保持独立。修改设备、线程或生成参数后需重新加载翻译模型。其他语言来自模型官方列表，日英中优先验证。</p>
      <details><summary>生成参数与缓存</summary><div class="grid">
        <label>CPU 线程<input type="number" min="1" max="64" bind:value={settings.threads}/></label><label>上下文 Token 上限<input type="number" min="512" max="16384" bind:value={settings.generation.contextSize}/></label>
        <label>翻译显存预留（MiB）<input type="number" min="128" max="8192" bind:value={settings.gpuReserveMb}/></label><p>Balanced 至少预留 2048 MiB。预留越少，游戏与其他 GPU 程序的剩余空间越小。</p>
        <label>输出 Token 上限<input type="number" min="1" max="4096" bind:value={settings.generation.maxTokens}/></label><label>温度<input type="number" min="0" max="2" step="0.1" bind:value={settings.generation.temperature}/></label>
        <label>Top P<input type="number" min="0.01" max="1" step="0.01" bind:value={settings.generation.topP}/></label><label>Top K<input type="number" min="1" max="200" bind:value={settings.generation.topK}/></label>
        <label>重复惩罚<input type="number" min="1" max="2" step="0.01" bind:value={settings.generation.repetitionPenalty}/></label><label>缓存条数<input type="number" min="0" max="4096" bind:value={settings.cacheCapacity}/></label>
      </div><label class="check"><input type="checkbox" bind:checked={settings.persistentCache}/> 退出后保留有界缓存（包含游戏文字）</label><Button size="sm" variant="outline" onclick={()=>run(api.clearTranslationCache)}>清空翻译缓存</Button></details>
      <label class="check"><input type="checkbox" bind:checked={settings.contextEnabled}/> 使用已确认的对白作为上下文</label>
      {#if settings.contextEnabled}<label>最多保留对白条数<input type="number" min="0" max="8" bind:value={settings.contextLines}/></label><p>选择游戏 Profile 后，在译文中点击“加入上下文”。短标签与 HUD 不附带剧情上下文；切换目标时清空。</p>{/if}
    {:else}
      <p>用 JSON 编辑游戏名称、源/目标语言、人物/地名/道具/技能术语、风格和背景。windowProcess 保存稳定进程名称，重新启动游戏后可手动选回此 Profile。</p>
      <div class="row"><Button size="sm" variant="outline" onclick={()=>run(async()=>addProfile())}>新增并绑定当前游戏</Button><Button size="sm" variant="outline" onclick={()=>run(importProfiles)}>导入 JSON</Button><Button size="sm" variant="outline" onclick={()=>run(exportProfiles)}>导出 JSON</Button></div>
      <textarea aria-label="游戏 Profile JSON" rows="18" bind:value={profileJson} spellcheck="false"></textarea>
      <p>glossary 条目格式：source（原文）、target（译文）、category（分类）。术语优先于上下文；所有游戏初始术语表为空。</p>
    {/if}
  </div>{/if}
  {#if error}<pre role="alert">{error}</pre>{/if}{#if notice}<p class="notice" aria-live="polite">{notice}</p>{/if}
  <footer class="row spread"><small>离线推理 · 翻译默认关闭</small><Button disabled={busy||!settings} onclick={()=>run(save)}>保存翻译设置</Button></footer>
</dialog>
<style>
  dialog{width:760px;padding:0;border:1px solid var(--border);border-radius:12px;background:var(--panel);color:var(--foreground)}dialog::backdrop{background:#0009;backdrop-filter:blur(4px)}header,footer{padding:18px 24px;border-bottom:1px solid var(--border)}h2{font-size:19px;margin:0}nav{display:flex;padding:0 24px;gap:24px;border-bottom:1px solid var(--border)}nav button{background:none;border:0;padding:14px 0;color:var(--muted);border-bottom:2px solid transparent}nav button.active{color:var(--primary);border-color:var(--primary)}.body{padding:18px 24px;max-height:65vh;overflow:auto}.grid{display:grid;grid-template-columns:1fr 1fr;gap:16px;margin:16px 0}label{display:flex;flex-direction:column;gap:7px;font-size:12px}.check{flex-direction:row;align-items:center;margin:16px 0}p,small{font-size:11px;line-height:1.7;color:var(--muted)}details{padding:14px 0;border-top:1px solid var(--border);font-size:12px}textarea{width:100%;resize:vertical;background:var(--background);color:var(--foreground);padding:12px;border:1px solid var(--border);border-radius:6px;font:12px/1.7 Consolas,monospace;margin-top:16px}pre{margin:12px 24px;color:#e2818b;font-size:11px;white-space:pre-wrap;overflow-wrap:anywhere}.notice{margin:10px 24px;color:var(--primary)}footer{border-top:1px solid var(--border);border-bottom:0}.row{flex-wrap:wrap}
</style>
