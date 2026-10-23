"""Read GGUF metadata/tensor kinds without loading tensors or requiring Python at runtime."""
import argparse, collections, json, struct

p = argparse.ArgumentParser()
p.add_argument('model')
p.add_argument('--output', required=True)
args = p.parse_args()
with open(args.model, 'rb') as f:
    def number(fmt):
        return struct.unpack('<' + fmt, f.read(struct.calcsize('<' + fmt)))[0]
    def string():
        n = number('Q')
        if n > 100_000_000: raise ValueError('GGUF string too large')
        return f.read(n).decode('utf-8')
    def value(t):
        formats = {0:'B', 1:'b', 2:'H', 3:'h', 4:'I', 5:'i', 6:'f', 7:'?', 10:'Q', 11:'q', 12:'d'}
        if t in formats: return number(formats[t])
        if t == 8: return string()
        if t == 9:
            kind, count = number('I'), number('Q')
            if count > 2_000_000: raise ValueError('GGUF array too large')
            vals = [value(kind) for _ in range(count)]
            return vals if count < 100 else {'count':count, 'elementType':kind}
        raise ValueError(f'Unknown GGUF type {t}')
    if f.read(4) != b'GGUF': raise ValueError('Not GGUF')
    version, nt, nk = number('I'), number('Q'), number('Q')
    metadata = {}
    for _ in range(nk):
        key = string()
        metadata[key] = value(number('I'))
    kinds = collections.Counter()
    for _ in range(nt):
        name, nd = string(), number('I')
        dims = [number('Q') for _ in range(nd)]
        kind, offset = number('I'), number('Q')
        kinds[kind] += 1
with open(args.output, 'w', encoding='utf-8') as f:
    json.dump({'ggufVersion':version, 'tensorCount':nt, 'tensorTypes':dict(kinds), 'metadata':metadata}, f, ensure_ascii=False, indent=2)
print(json.dumps({'tensorTypes':dict(kinds), 'architecture':metadata.get('general.architecture'), 'chatTemplate':metadata.get('tokenizer.chat_template')}, ensure_ascii=False))
