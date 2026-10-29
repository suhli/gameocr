<script lang="ts">
  import {onMount} from 'svelte';
  import {listen} from '@tauri-apps/api/event';
  import {api} from '../services/api';
  import {Button} from '$lib/components/ui/button';
  import type {OverlaySettings} from '../types/overlay';
  let draft=$state<OverlaySettings|null>(null),saving=$state(false),message=$state(''),error=$state(''),status=$state('');
  onMount(()=>{
    let disposed=false;let unlisten:(()=>void)|undefined;
    void api.overlaySettings().then(s=>draft=s).catch(e=>error=String(e));
    void api.overlaySnapshot().then(s=>status=s.reason).catch(e=>error=String(e));
    void listen<string>('overlay-status',event=>status=event.payload).then(fn=>{if(disposed)fn();else unlisten=fn;}).catch(e=>error=String(e));
    return ()=>{disposed=true;unlisten?.();};
  });
  async function save(){if(!draft)return;saving=true;error='';try{await api.updateOverlaySettings($state.snapshot(draft));message=draft.overlayEnabled?'游戏字幕已启用：切回目标窗口显示当前有效译文。若已过期，请重新识别。':'游戏字幕已关闭，预览译文不受影响。';}catch(e){error=String(e);}finally{saving=false;}}
</script>
<section aria-label="游戏翻译字幕设置"><h3>游戏翻译字幕</h3><p>在前台游戏窗口上显示完整译文。支持普通窗口和无边框窗口；不支持 ADB 或独占全屏。样式单独保存，不需要卸载模型。</p>
{#if draft}
  <label class="check"><input type="checkbox" bind:checked={draft.overlayEnabled}/> 启用实时翻译字幕</label>
  <div class="fields">
    <label>背景不透明度<input type="number" min="0.1" max="0.9" step="0.05" bind:value={draft.overlayOpacity}/></label>
    <label>磨砂视觉强度<input type="number" min="0" max="16" step="1" bind:value={draft.overlayBlur}/></label>
    <label>字幕字号<input type="number" min="12" max="36" step="1" bind:value={draft.overlayFontSize}/></label>
    <label>最大宽度比例<input type="number" min="0.15" max="0.8" step="0.05" bind:value={draft.overlayMaxWidthRatio}/></label>
    <label>字幕内边距<input type="number" min="2" max="20" step="1" bind:value={draft.overlayPadding}/></label>
    <label>字幕圆角<input type="number" min="0" max="24" step="1" bind:value={draft.overlayCornerRadius}/></label>
  </div>
  <label class="check"><input type="checkbox" bind:checked={draft.overlayFollowWindow}/> 跟随游戏窗口</label>
  <label class="check"><input type="checkbox" bind:checked={draft.overlayAutoHideWhenStale}/> 无新识别结果时提前隐藏</label>
  {#if draft.overlayAutoHideWhenStale}<label>无新结果的保留上限 (ms)<input type="number" min="2000" max="30000" step="1000" bind:value={draft.overlayStaleMs}/></label>{/if}
  <p>半透明渐变模拟磨砂；不会真正模糊游戏画面。源文字改变、停止或暂停识别时立即清除。无新结果最多保留 30 秒；关闭跟随后，窗口移位会隐藏字幕。</p>
  <Button disabled={saving} onclick={save}>{saving?'应用中…':'应用字幕设置'}</Button>
{/if}
{#if message}<p role="status">{message}</p>{/if}{#if error}<p role="alert">{error}</p>{/if}
{#if status}<p aria-label="字幕状态">{status}</p>{/if}
</section>
<style>section{padding:20px 0;border-bottom:1px solid var(--border)}h3{margin:0 0 10px;font-size:13px;font-weight:600}p{color:var(--muted);font-size:11px;line-height:1.7}label{display:block;font-size:11px;color:var(--muted);margin:12px 0}label input{margin-top:7px;font-size:12px}.check{display:flex;align-items:center;gap:8px}.check input{margin:0}.fields{display:grid;grid-template-columns:1fr 1fr;gap:18px}.fields label{margin:9px 0}[role=alert]{color:var(--danger,#f87171)}</style>
