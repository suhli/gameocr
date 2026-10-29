import type {OverlayItem,OverlaySettings,OverlaySnapshot} from '../types/overlay';
import type {OcrBlock} from '../types';
export const overlayDefaults:OverlaySettings={overlayEnabled:false,overlayOpacity:.55,overlayBlur:8,overlayFontSize:16,overlayMaxWidthRatio:.45,overlayPadding:8,overlayCornerRadius:10,overlayFollowWindow:true,overlayAutoHideWhenStale:true,overlayStaleMs:15000};
const bounded=(value:number,min:number,max:number,fallback:number)=>Number.isFinite(value)?Math.min(max,Math.max(min,value)):fallback;
export function safeStyle(s:OverlaySettings):OverlaySettings{return {...s,overlayOpacity:bounded(s.overlayOpacity,.1,.9,.55),overlayBlur:bounded(s.overlayBlur,0,16,8),overlayFontSize:bounded(s.overlayFontSize,12,36,16),overlayMaxWidthRatio:bounded(s.overlayMaxWidthRatio,.15,.8,.45),overlayPadding:bounded(s.overlayPadding,2,20,8),overlayCornerRadius:bounded(s.overlayCornerRadius,0,24,10)};}
type GeometryBlock={bbox?:OcrBlock['bbox']|null;polygon?:OcrBlock['polygon']};
export function blockBounds(b:GeometryBlock,w:number,h:number):[number,number,number,number]|null{
  const p=b.polygon;const raw=p&&p.length>=3&&p.length<=64&&p.every(p=>Number.isFinite(p.x)&&Number.isFinite(p.y))?[Math.min(...p.map(p=>p.x)),Math.min(...p.map(p=>p.y)),Math.max(...p.map(p=>p.x)),Math.max(...p.map(p=>p.y))]:b.bbox;
  if(!raw||![w,h,...raw].every(Number.isFinite)||w<=0||h<=0||raw[2]<=raw[0]||raw[3]<=raw[1])return null;
  const box:[number,number,number,number]=[Math.max(0,raw[0]),Math.max(0,raw[1]),Math.min(w,raw[2]),Math.min(h,raw[3])];return box[2]>box[0]&&box[3]>box[1]?box:null;
}
export function unionBounds(blocks:GeometryBlock[],w:number,h:number){
  if(!blocks.length||blocks.length>32)return null;const boxes=blocks.map(b=>blockBounds(b,w,h));if(boxes.some(b=>!b))return null;
  const valid=boxes as [number,number,number,number][];const box:[number,number,number,number]=[Math.min(...valid.map(b=>b[0])),Math.min(...valid.map(b=>b[1])),Math.max(...valid.map(b=>b[2])),Math.max(...valid.map(b=>b[3]))];
  const area=(box[2]-box[0])*(box[3]-box[1]),sum=valid.reduce((sum,b)=>sum+(b[2]-b[0])*(b[3]-b[1]),0);
  return valid.length>1&&(area>sum*4||area>w*h*.45)?null:box;
}
export function mapBox(bbox:[number,number,number,number],sourceW:number,sourceH:number,viewportW:number,viewportH:number){
  const b=blockBounds({bbox,polygon:null},sourceW,sourceH);if(!b||![viewportW,viewportH].every(n=>Number.isFinite(n)&&n>0))return null;
  return {x:b[0]/sourceW*viewportW,y:b[1]/sourceH*viewportH,width:(b[2]-b[0])/sourceW*viewportW,height:(b[3]-b[1])/sourceH*viewportH};
}
export interface SubtitleLayout {id:string;text:string;x:number;y:number;width:number;height:number;fontSize:number;lines:number;}
export function layoutSubtitles(items:OverlayItem[],sourceW:number,sourceH:number,w:number,h:number,settings:OverlaySettings):SubtitleLayout[]{
  const s=safeStyle(settings),result:SubtitleLayout[]=[];if(w<64||h<48)return result;
  for(const item of items.slice(0,64)){
    const b=mapBox(item.bbox,sourceW,sourceH,w,h);if(!b||!item.text.trim())continue;
    const pad=s.overlayPadding,maxW=Math.min(w-8,w*s.overlayMaxWidthRatio),width=Math.min(maxW,Math.max(b.width,Math.min(160,maxW)));
    const maxH=Math.min(h*.28,200),units=Array.from(item.text.slice(0,8192)).reduce((n,c)=>n+(c.charCodeAt(0)>255?1:.6),0);
    let fontSize=s.overlayFontSize;
    const estimate=(size:number)=>Math.max(1,Math.ceil(units/Math.max(1,(width-pad*3)/size)));
    while(fontSize>12&&estimate(fontSize)*fontSize*1.45+pad*2>maxH)fontSize--;
    const lines=Math.max(1,Math.min(6,estimate(fontSize),Math.floor((maxH-pad*2)/(fontSize*1.45))));
    const height=Math.min(maxH,lines*fontSize*1.45+pad*2+2);
    const x=Math.max(4,Math.min(w-width-4,b.x+b.width/2-width/2));
    let y=Math.max(4,Math.min(h-height-4,b.y+b.height/2-height/2));
    const overlaps=(yy:number)=>result.some(r=>x<r.x+r.width+4&&x+width+4>r.x&&yy<r.y+r.height+4&&yy+height+4>r.y);
    if(overlaps(y)){
      const options=[b.y+b.height+6,b.y-height-6];const alternative=options.find(v=>v>=4&&v+height<=h-4&&!overlaps(v));
      if(alternative===undefined)continue;y=alternative;
    }
    result.push({id:item.id,text:item.text.slice(0,8192),x,y,width,height,fontSize,lines});
  }
  return result;
}
// Serial also orders empty invalidations/settings changes; an old result cannot
// restore captions after an event-driven clear or renderer initialization race.
export function acceptSnapshot(current:OverlaySnapshot|null,next:OverlaySnapshot):OverlaySnapshot|null{
  if(current&&next.serial<=current.serial)return current;
  if(current?.identity&&next.identity&&current.targetId===next.targetId){
    const a=current.identity,b=next.identity;
    if(b.generation<a.generation||(b.generation===a.generation&&b.epoch<a.epoch)||(b.generation===a.generation&&b.epoch===a.epoch&&b.revision<a.revision))return current;
  }
  return next;
}
