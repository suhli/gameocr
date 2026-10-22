"""Optional offline model variant; preserves full recognizer for safe fallback."""
import argparse,hashlib,json,shutil
from pathlib import Path
import onnx
from onnx import helper as h,TensorProto as T
import numpy as np
p=argparse.ArgumentParser();p.add_argument('manifest');p.add_argument('destination');a=p.parse_args()
src=Path(a.manifest).resolve();target=Path(a.destination).resolve()
if target.exists():raise SystemExit('Destination must be new; original models are never modified')
m=json.loads(src.read_text(encoding='utf-8-sig'));target.mkdir(parents=True)
for f in m['files']:
    file=src.parent/f['path']
    assert hashlib.sha256(file.read_bytes()).hexdigest()==f['sha256'],'Source checksum mismatch'
    out=target/f['path'];out.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(file,out)
rec=next(f for f in m['files'] if f['role']=='recognizer');model=onnx.load(target/rec['path'])
if next(o.version for o in model.opset_import if not o.domain)<13:model=onnx.version_converter.convert_version(model,13)
prob=model.graph.output[0].name
names=['gameocr_ctc_ids','gameocr_ctc_scores','gameocr_ctc_valid']
assert not any(n.startswith('gameocr_ctc') for node in model.graph.node for n in node.output)
model.graph.initializer.extend([onnx.numpy_helper.from_array(np.array(0,dtype=np.float32),'gameocr_ctc_zero'),onnx.numpy_helper.from_array(np.array(1.00001,dtype=np.float32),'gameocr_ctc_limit')])
classes=model.graph.output[0].type.tensor_type.shape.dim[2].dim_value
assert classes>1,'Recognizer must declare its dictionary dimension'
# ORT CUDA falls back to CPU for select_last_index=1. Reverse the class axis,
# select the first maximum on GPU, then map the index back to preserve last ties.
model.graph.initializer.extend([onnx.numpy_helper.from_array(np.arange(classes-1,-1,-1,dtype=np.int64),'gameocr_ctc_reverse'),onnx.numpy_helper.from_array(np.array(classes-1,dtype=np.int64),'gameocr_ctc_last')])
nodes=[h.make_node('Gather',[prob,'gameocr_ctc_reverse'],['gameocr_ctc_reversed'],axis=2),
       h.make_node('ArgMax',['gameocr_ctc_reversed'],['gameocr_ctc_reverse_id'],axis=2,keepdims=0,select_last_index=0),
       h.make_node('Sub',['gameocr_ctc_last','gameocr_ctc_reverse_id'],[names[0]]),
       h.make_node('GreaterOrEqual',[prob,'gameocr_ctc_zero'],['gameocr_ctc_lower']),
       h.make_node('LessOrEqual',[prob,'gameocr_ctc_limit'],['gameocr_ctc_upper']),
       h.make_node('And',['gameocr_ctc_lower','gameocr_ctc_upper'],['gameocr_ctc_ok']),
       h.make_node('Cast',['gameocr_ctc_ok'],['gameocr_ctc_float'],to=T.FLOAT)]
opset=next(o.version for o in model.opset_import if not o.domain)
for op,input_,output in [('ReduceMax',prob,names[1]),('ReduceMin','gameocr_ctc_float',names[2])]:
    if opset>=18:
        model.graph.initializer.append(onnx.numpy_helper.from_array(np.array([2],dtype=np.int64),output+'_axes'))
        nodes.append(h.make_node(op,[input_,output+'_axes'],[output],keepdims=0))
    else:nodes.append(h.make_node(op,[input_],[output],axes=[2],keepdims=0))
for node in nodes:node.name=node.output[0]
model.graph.node.extend(nodes);del model.graph.output[:]
model.graph.output.extend([h.make_tensor_value_info(names[0],T.INT64,['batch','steps']),h.make_tensor_value_info(names[1],T.FLOAT,['batch','steps']),h.make_tensor_value_info(names[2],T.FLOAT,['batch','steps'])])
onnx.checker.check_model(model);file=target/'recognizer-ctc.onnx';onnx.save(model,file)
m['id']+='-compact-ctc-experimental';m['name']+=' Compact CTC (experimental)'
m['imported']=True;m['source']='local compact CTC experiment';m['installed']=True
m['files'].append({'role':'recognizer-ctc','path':file.name,'size':file.stat().st_size,'sha256':hashlib.sha256(file.read_bytes()).hexdigest()})
(target/(m['id']+'.json')).write_text(json.dumps(m,ensure_ascii=False,indent=2),encoding='utf-8')
print(target/(m['id']+'.json'))
