"""Local design review only. Serves unchanged UI 2 assets against synthetic read-only data."""
import json, pathlib, time
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from urllib.parse import urlparse
ROOT = pathlib.Path(__file__).resolve().parents[3]
HERE = pathlib.Path(__file__).resolve().parent
summary = dict(request_count=24860, failure_count=186, estimated_cost_usd=42.86, average_latency_ms=342, input_tokens=2200000, output_tokens=640000, fallback_rate=.012, denied_count=42)
projects = [dict(id='demo-project', name='Customer support', description='Production assistants', enabled=True, service_names=[], created_at='2026-09-01T00:00:00Z')]
keys = [dict(id='demo-key', key_prefix='rk_live_demo', name='Support worker', project_id='demo-project', disabled=False, revoked_at=None, expires_at='2026-08-01T00:00:00Z', created_at='2026-07-01T00:00:00Z', policy={}, guardrail_policy={})]
health = [dict(name='LiteLLM', provider='litellm', status='healthy', request_count=21000, error_count=20, timeout_count=0, fallback_count=0, total_latency_ms=6000000),dict(name='Document service', provider='internal-service', status='degraded', request_count=3860, error_count=166, timeout_count=150, fallback_count=30,total_latency_ms=1700000)]
class Handler(SimpleHTTPRequestHandler):
 def do_GET(self):
  p=urlparse(self.path).path
  if p.startswith('/admin-ui/admin/') or p in ['/admin-ui/auth/config','/admin-ui/auth/session','/admin-ui/readyz']:
   obj=[]; status=200
   if p.endswith('/auth/config'): obj={'oidc_enabled':False}
   elif p.endswith('/auth/session'): obj={'authenticated':True,'csrf_token':'synthetic-demo','member':{'status':'active','roles':['admin'],'display_name':'Design preview'},'service_memberships':[],'project_memberships':[]}
   elif p.endswith('/readyz'):
    status=503 if (HERE/'degraded.flag').exists() else 200
    obj={'status':'not_ready' if status==503 else 'ready'}
   elif p.endswith('/usage/dashboard'): obj={'summary':summary,'timeseries':[{'bucket_start':f'2026-09-0{i+1}T00:00:00Z','summary':dict(summary,request_count=3000+i*220)} for i in range(5)],'breakdowns':{k:[] for k in ['projects','keys','services','providers','models','tasks','endpoints']},'unused_keys':[],'service_timeseries':[]}
   elif p.endswith('/usage/events'): obj={'rows':[],'total':0,'has_more':False}
   elif p.endswith('/usage/filter-values'): obj={'values':[]}
   elif p.endswith('/projects'): obj=projects
   elif p.endswith('/keys'): obj=keys
   elif p.endswith('/provider-health'): obj=health
   elif p.endswith('/litellm-passthrough'): obj={}
   elif p.endswith('/guardrails/summary'): obj=[]
   elif p.endswith('/auth-settings') or p.endswith('/studio-connection'): obj={}
   if p.endswith('/projects') and (HERE/'slow.flag').exists(): time.sleep(6)
   data=json.dumps(obj).encode(); self.send_response(status); self.send_header('Content-Type','application/json'); self.end_headers(); self.wfile.write(data); return
  if p in ['/admin-ui','/admin-ui/']: f=ROOT/'crates/gateway-api/src/static/admin-ui/index.html'
  elif p.startswith('/admin-ui/'): f=ROOT/'crates/gateway-api/src/static/admin-ui'/p.removeprefix('/admin-ui/')
  else: f=HERE/('index.html' if p=='/' else p.lstrip('/'))
  if not f.is_file(): self.send_error(404); return
  self.send_response(200); self.send_header('Content-Type',self.guess_type(str(f))); self.end_headers(); self.wfile.write(f.read_bytes())
 def do_POST(self): self.send_error(405,'Read-only design fixture')
 do_PATCH=do_POST
 do_DELETE=do_POST
 def log_message(self,*args): pass
ThreadingHTTPServer(('127.0.0.1',18430),Handler).serve_forever()
