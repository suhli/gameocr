// Import independently annotated real sequences; never invent labels. Input:
// {scenarios:[{name,frames:[{image,atMs,generation,groundTruth:[{text,bbox}]}]}]}.
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';import {createHash} from 'node:crypto';
const [input,destination]=process.argv.slice(2);assert(input&&destination,'Usage: node scripts/import-temporal-corpus.mjs annotations.json NEW_DIRECTORY');
const source=path.resolve(input),root=path.resolve(destination);assert(!fs.existsSync(root),'Destination must be new');
const data=JSON.parse(fs.readFileSync(source,'utf8').replace(/^\uFEFF/,''));assert(Array.isArray(data.scenarios)&&data.scenarios.length);
const files=[];let id=0;
for(const scene of data.scenarios){assert(typeof scene.name==='string'&&Array.isArray(scene.frames)&&scene.frames.length>=2);scene.source='user-supplied';let previous=-1;
 for(const frame of scene.frames){assert(Number.isFinite(frame.atMs)&&frame.atMs>=previous,'Frame times must be ordered');previous=frame.atMs;assert(Array.isArray(frame.groundTruth),'Missing annotations');
  for(const line of frame.groundTruth){assert(typeof line.text==='string'&&Array.isArray(line.bbox)&&line.bbox.length===4&&line.bbox.every(Number.isFinite)&&line.bbox[2]>line.bbox[0]&&line.bbox[3]>line.bbox[1],'Invalid text box');}
  const file=path.resolve(path.dirname(source),frame.image);assert(fs.statSync(file).isFile());const ext=path.extname(file).toLowerCase();assert(['.png','.jpg','.jpeg','.ppm'].includes(ext));
  frame.sha256=createHash('sha256').update(fs.readFileSync(file)).digest('hex');frame.image=`frame-${id++}${ext}`;files.push({file,name:frame.image});
 }
}
fs.mkdirSync(root,{recursive:true});for(const file of files)fs.copyFileSync(file.file,path.join(root,file.name));
data.schemaVersion=1;data.source='user-supplied sequences and annotations; provenance not independently verified';fs.writeFileSync(path.join(root,'corpus.json'),JSON.stringify(data,null,2)+'\n');console.log(path.join(root,'corpus.json'));
