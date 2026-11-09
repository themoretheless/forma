from http.server import ThreadingHTTPServer,SimpleHTTPRequestHandler
from pathlib import Path
import json
import argparse
parser=argparse.ArgumentParser()
parser.add_argument('--wasm-dir',type=Path,required=True)
parser.add_argument('--result',type=Path,required=True)
parser.add_argument('--port',type=int,default=8769)
args=parser.parse_args()
root=Path(__file__).resolve().parents[3]
class Handler(SimpleHTTPRequestHandler):
 def __init__(self,*a,**kw): super().__init__(*a,directory=str(root),**kw)
 def end_headers(self):
  self.send_header('Cache-Control','no-store');super().end_headers()
 def do_GET(self):
  if self.path.startswith('/wasm/'):
   p=args.wasm_dir/self.path.split('/')[-1];data=p.read_bytes();self.send_response(200);self.send_header('Content-Type','application/wasm' if p.suffix=='.wasm' else 'application/javascript');self.end_headers();self.wfile.write(data)
  elif self.path.endswith('vector.wgsl?raw'):
   data=('export default '+json.dumps((root/'crates/forma/src/vector.wgsl').read_text())+';').encode();self.send_response(200);self.send_header('Content-Type','application/javascript');self.end_headers();self.wfile.write(data)
  else:super().do_GET()
 def do_POST(self):
  data=self.rfile.read(int(self.headers['Content-Length']));args.result.write_bytes(data);self.send_response(200);self.end_headers()
ThreadingHTTPServer(('127.0.0.1',args.port),Handler).serve_forever()
