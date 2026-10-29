<script lang="ts">
  import type {SubtitleLayout} from '../lib/overlayGeometry';
  import type {OverlaySettings} from '../types/overlay';
  let {item,style}:{item:SubtitleLayout;style:OverlaySettings}=$props();
</script>
<div class="subtitle" data-subtitle-id={item.id} style:left={`${item.x}px`} style:top={`${item.y}px`} style:width={`${item.width}px`} style:height={`${item.height}px`} style:font-size={`${item.fontSize}px`} style:padding={`${style.overlayPadding}px ${style.overlayPadding*1.5}px`} style:border-radius={`${style.overlayCornerRadius}px`} style:background-color={`rgba(20,26,36,${style.overlayOpacity})`} style:--frost={0.025+style.overlayBlur/400} style:backdrop-filter={`blur(${style.overlayBlur}px)`}>
  <span style:-webkit-line-clamp={item.lines}>{item.text}</span>
</div>
<style>
  .subtitle{position:absolute;box-sizing:border-box;overflow:hidden;color:#f7f9fc;line-height:1.45;text-align:center;border:1px solid #ffffff1f;background-image:linear-gradient(135deg,rgba(255,255,255,var(--frost)),transparent 60%,#a8c5e108);box-shadow:0 3px 14px #0003;text-shadow:0 1px 2px #0009;pointer-events:none}
  span{display:-webkit-box;-webkit-box-orient:vertical;overflow:hidden;overflow-wrap:anywhere;word-break:normal;white-space:pre-wrap;max-width:100%}
</style>
