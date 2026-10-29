<script lang="ts">
  import { ScanLine,Monitor,Maximize2,ZoomIn,ZoomOut,Image,Focus } from '@lucide/svelte';
  import type { Preview,OcrResult } from '../types';
  import { boxStyle,fitWidth } from '../lib/geometry';
  import type {TranslationEvent} from '../types/translation';
  import type {OverlaySettings} from '../types/overlay';
  import {previewSubtitles} from '../lib/translationOverlay';
  import {layoutSubtitles,overlayDefaults,safeStyle} from '../lib/overlayGeometry';
  import OverlaySubtitle from '../overlay/OverlaySubtitle.svelte';
  let {frame,result=null,translation=null,overlaySettings=overlayDefaults,showTranslations=true,selected=null,showBoxes=true,onSelect}:{frame:Preview|null;result?:OcrResult|null;translation?:TranslationEvent|null;overlaySettings?:OverlaySettings;showTranslations?:boolean;selected?:string|null;showBoxes?:boolean;onSelect:(id:string)=>void}=$props();
  let width=$state(800),height=$state(500),zoom=$state(0);
  const imageWidth=$derived(frame?(zoom?frame.width*zoom:fitWidth(frame.width,frame.height,width,height)):1);
  const boxes=$derived(showBoxes&&result&&frame?.frameId===result.frameId?result.blocks.filter(b=>b.bbox):[]);
  const style=$derived(safeStyle(overlaySettings));
  const subtitles=$derived(showTranslations?previewSubtitles(frame,result,translation):[]);
  const layouts=$derived(frame&&result?layoutSubtitles(subtitles,result.sourceWidth,result.sourceHeight,imageWidth,imageWidth*frame.height/frame.width,style):[]);
</script>
<div class="preview-shell">
  <div class="viewport" bind:clientWidth={width} bind:clientHeight={height}>
    {#if frame}
      <div class="image-stage" style:width={`${imageWidth}px`} style:aspect-ratio={`${frame.width}/${frame.height}`}>
        <img src={frame.dataUrl} alt={`捕获的原始窗口画面，第 ${frame.frameId} 帧`} draggable="false"/>
        {#each boxes as block}
          <button class="bbox" class:chosen={selected===block.id} class:region={['image','chart','table'].includes(block.label??'')} style={boxStyle(block.bbox!,frame.width,frame.height)} onclick={()=>onSelect(block.id)} aria-label={block.text||block.label||'文字区域'} title={block.text}>
            <span>{block.label??'text'}</span>
          </button>
        {/each}
        <div class="preview-subtitles" aria-label="预览画面译文">
          {#each layouts as item (item.id)}<OverlaySubtitle {item} {style}/>{/each}
        </div>
      </div>
    {:else}
      <div class="awaiting">
        <div class="reticle"><ScanLine size={40}/><i class="corner tl"></i><i class="corner tr"></i><i class="corner bl"></i><i class="corner br"></i></div>
        <h2>让画面里的文字，清晰可见</h2>
        <p>从左侧选择游戏或模拟器窗口，开始捕获画面。</p>
        <div class="steps"><span><Monitor/> 选择窗口</span><b>→</b><span><Image/> 捕获画面</span><b>→</b><span><Focus/> 本地识别</span></div>
        <small>所有图像与推理均在本机处理</small>
      </div>
    {/if}
  </div>
  <div class="preview-bottom row spread">
    <span class="mono muted">{frame?`${frame.width} × ${frame.height}  ·  FRAME ${frame.frameId}`:'NO SIGNAL'} {#if frame&&showTranslations}<span class="subtitle-count">{layouts.length?` · 译文 ${layouts.length} 项`:' · 此帧暂无可定位的完整译文'}</span>{/if}</span>
    <div class="row"><button class="icon-btn" aria-label="缩小" disabled={!frame} onclick={()=>zoom=Math.max(.25,(zoom||imageWidth/(frame?.width||1))-.25)}><ZoomOut/></button><span class="mono">{zoom?`${Math.round(zoom*100)}%`:'适应窗口'}</span><button class="icon-btn" aria-label="放大" disabled={!frame} onclick={()=>zoom=Math.min(3,(zoom||imageWidth/(frame?.width||1))+.25)}><ZoomIn/></button><button class="icon-btn" aria-label="适应窗口" onclick={()=>zoom=0}><Maximize2/></button></div>
  </div>
</div>
<style>
  .preview-subtitles{position:absolute;inset:0;overflow:hidden;pointer-events:none;z-index:2}.subtitle-count{font-family:inherit}
  .preview-shell{height:100%;min-height:0;display:flex;flex-direction:column}.viewport{flex:1;min-height:0;overflow:auto;display:flex;align-items:center;justify-content:center;padding:24px;background-color:var(--background);background-image:radial-gradient(var(--grid) 1px,transparent 1px);background-size:20px 20px}.image-stage{position:relative;flex-shrink:0;margin:auto;box-shadow:0 12px 50px #0005}.image-stage img{display:block;width:100%;height:100%;object-fit:fill}.bbox{position:absolute;padding:0;background:#54ebba0c;border:1.5px solid #35c998;border-radius:2px;transition:background .1s;min-width:1px;min-height:1px}.bbox:hover,.bbox.chosen{border-color:#fff;background:#54ebba28;box-shadow:0 0 0 2px #35c998}.bbox.region{border-color:#c598fa;border-style:dashed}.bbox span{position:absolute;left:-1px;top:-17px;background:#153d33;color:#92f4d1;font-size:9px;line-height:16px;padding:0 4px;font-family:Consolas,monospace;opacity:0;pointer-events:none}.bbox:hover span,.bbox.chosen span{opacity:1}.preview-bottom{height:42px;flex-shrink:0;border-top:1px solid var(--border);padding:0 14px;font-size:10px;background:var(--panel)}.awaiting{text-align:center;color:var(--muted)}.reticle{position:relative;display:grid;place-items:center;width:104px;height:94px;margin:0 auto 30px;color:#43586c}.reticle :global(svg){width:40px;height:40px}.corner{position:absolute;width:18px;height:18px;border-color:#3b5b57;border-style:solid}.tl{top:0;left:0;border-width:1px 0 0 1px}.tr{top:0;right:0;border-width:1px 1px 0 0}.bl{bottom:0;left:0;border-width:0 0 1px 1px}.br{bottom:0;right:0;border-width:0 1px 1px 0}h2{font-size:19px;font-weight:500;color:var(--foreground);letter-spacing:.04em;margin:0 0 12px}p{font-size:12px}.steps{display:flex;align-items:center;justify-content:center;gap:13px;margin:34px 0 26px;font-size:11px}.steps span{display:flex;align-items:center;gap:6px}.steps b{font-weight:400;color:var(--border)}small{font-size:10px;color:var(--muted);opacity:.7}
</style>
