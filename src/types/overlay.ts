export interface OverlaySettings {
  overlayEnabled:boolean; overlayOpacity:number; overlayBlur:number; overlayFontSize:number;
  overlayMaxWidthRatio:number; overlayPadding:number; overlayCornerRadius:number;
  overlayFollowWindow:boolean; overlayAutoHideWhenStale:boolean; overlayStaleMs:number;
}
export interface OverlayItem {id:string;text:string;bbox:[number,number,number,number];sourceBlockIds:string[];}
export interface OverlaySnapshot {
  serial:number; settings:OverlaySettings;
  identity:{frameId:number;generation:number;epoch:number;revision:number}|null;
  targetId:string|null; sourceWidth:number; sourceHeight:number;
  geometry:{rect:[number,number,number,number];clientRect:[number,number,number,number];dpi:number;mapping:string;foreground:boolean;visible:boolean}|null;
  items:OverlayItem[]; visible:boolean; reason:string;
}
