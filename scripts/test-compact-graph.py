"""Tests the actual appended graph on malformed non-max values and ties.
CPU execution isolates graph semantics; CUDA model execution is benchmarked
separately. This does not claim GPU timings or test real game quality.
"""
import json
from pathlib import Path
import numpy as np
import onnx
from onnx import helper as h,TensorProto as T
import onnxruntime as ort
model=onnx.load('.cache/optimized-models/small-compact-ctc/recognizer-ctc.onnx')
nodes=list(model.graph.node);index=next(i for i,n in enumerate(nodes) if 'gameocr_ctc_ids' in n.output)
prob=nodes[index].input[0]
graph=h.make_graph(nodes[index:],'compact-validation',[h.make_tensor_value_info(prob,T.FLOAT,[1,1,4])],list(model.graph.output),[i for i in model.graph.initializer if i.name.startswith('gameocr_ctc_')])
test=h.make_model(graph,opset_imports=list(model.opset_import));test.ir_version=model.ir_version;onnx.checker.check_model(test)
options=ort.SessionOptions();options.intra_op_num_threads=1
session=ort.InferenceSession(test.SerializeToString(),sess_options=options,providers=['CPUExecutionProvider'])
checks=[]
for name,values,valid,expected in [
    ('tie-last',[0,.1,.8,.8],True,3),('blank',[.9,.1,0,0],True,0),
    ('nonmax-nan',[float('nan'),.9,.1,0],False,None),('nonmax-negative',[-.01,.9,.1,0],False,None),
    ('infinity',[float('inf'),.9,.1,0],False,None),('over-one',[0,1.1,.1,0],False,None),
    ('negative-zero',[0.,-0.,0.,-0.],True,None),
]:
    ids,scores,ok=session.run(None,{prob:np.array(values,dtype=np.float32).reshape(1,1,4)})
    assert (ok.item()==1.)==valid,name
    if expected is not None:assert ids.item()==expected,name
    if name=='negative-zero':assert scores.item()==0. # Rust deliberately falls back
    checks.append(name)
Path('docs/test-results/compact-ctc-validation.json').write_text(json.dumps({'scope':'Appended graph on CPU; Rust rejects zero maxima and applies blank/repeat semantics','checks':checks},indent=2))
print(f'{len(checks)} compact graph checks passed')
