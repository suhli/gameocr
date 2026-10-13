export function boxStyle(bbox:[number,number,number,number],width:number,height:number):string{
  if(width<=0||height<=0)return 'display:none';
  const [x1,y1,x2,y2]=bbox;
  return `left:${x1/width*100}%;top:${y1/height*100}%;width:${(x2-x1)/width*100}%;height:${(y2-y1)/height*100}%`;
}
export function fitWidth(imageWidth:number,imageHeight:number,containerWidth:number,containerHeight:number):number{return Math.max(1,Math.min(containerWidth-48,(containerHeight-48)*imageWidth/imageHeight));}
