import argparse
import json
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

parser=argparse.ArgumentParser()
parser.add_argument('summary',type=Path)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
rows=json.loads(args.summary.read_text())['sizes']
labels=[f"{r['body_mib']} MiB" for r in rows]
x=np.arange(len(rows))
number=lambda key:np.array([r.get(key) if r.get(key) is not None else np.nan for r in rows],dtype=float)
plt.rcParams.update({'font.size':10,'svg.fonttype':'none','axes.spines.top':False,'axes.spines.right':False})
fig,axes=plt.subplots(3,1,figsize=(11,10),sharex=True,gridspec_kw={'height_ratios':[1.2,1.2,1]})
blue,green,orange,red='#286fbd','#127a65','#c58424','#c84148'
ax=axes[0]
stable=number('largest_three_resource_stable_slot_setting')
failed=number('adjacent_setting')
bound=number('arithmetic_complete_bodies')
ax.scatter(x-.12,bound,s=45,marker='s',color=blue,label='Complete-body byte upper bound')
ax.scatter(x,stable,s=55,marker='o',color=green,label='Largest configuration with 3/3 resource-stable trials')
ax.scatter(x+.12,failed,s=60,marker='^',color=red,label='Adjacent setting with observed OOM')
for at,value in zip(x,stable):
    if np.isfinite(value):ax.annotate(f'{value:.0f}',(at,value),xytext=(0,-16),textcoords='offset points',ha='center',color=green)
ax.set_yscale('log',base=2)
ax.set_ylabel('Count per pod (log₂)')
ax.set_title('Configured slot boundary and complete-body capacity are different measures',loc='left',fontweight='bold',pad=30)
ax.legend(loc='lower left',bbox_to_anchor=(0,1.0),fontsize=7.5,frameon=False,ncol=3)
ax.grid(axis='y',alpha=.15)
ax=axes[1]
rss=number('stable_rss_peak_mib');cgroup=number('stable_cgroup_peak_mib')
ax.bar(x-.18,rss,width=.34,color=blue,label='Highest process RSS')
ax.bar(x+.18,cgroup,width=.34,color=orange,label='Highest cgroup peak')
ax.axhline(1024,color=red,linestyle='--',linewidth=1,label='1 GiB pod memory limit')
for at,value in zip(x+.18,cgroup):
    if np.isfinite(value):ax.text(at,value-50,f'{value:.0f}',ha='center',fontsize=8,color='white',fontweight='bold')
ax.set_ylim(0,max(1150,float(np.nanmax(cgroup))*1.12) if np.isfinite(cgroup).any() else 1150)
ax.set_ylabel('MiB per pod')
ax.set_title('Peak memory at the largest resource-stable setting',loc='left',fontweight='bold',pad=30)
ax.legend(loc='lower left',bbox_to_anchor=(0,1.0),fontsize=8,frameon=False,ncol=3)
ax.grid(axis='y',alpha=.15)
ax=axes[2]
latency=number('stable_success_p95_max_ms')/1000
ax.bar(x,latency,width=.45,color=green)
for at,value in zip(x,latency):
    if np.isfinite(value):ax.text(at,value+.4,f'{value:.1f} s',ha='center',fontsize=9)
ax.set_ylim(0,float(np.nanmax(latency))*1.25 if np.isfinite(latency).any() else 15)
ax.set_ylabel('Seconds')
ax.set_title('Worst successful-response p95 across resource-stable confirmation trials',loc='left',fontweight='bold',pad=30)
ax.set_xticks(x,labels)
ax.grid(axis='y',alpha=.15)
fig.suptitle('Native amd64 · Two Gateway pods · Each limited to 2 CPUs / 1 GiB · 512 MiB body budget',x=.09,ha='left',fontsize=13,fontweight='bold')
edge=min(rows,key=lambda r:1024-r['stable_cgroup_peak_mib'])
fig.text(.09,.025,'Per-size native hosts and offered cohorts are recorded in the report; cross-size differences are not solely payload effects.\nMemory values are individual-pod maxima, not two-pod sums. Timing includes the 10 s read pause and 2 s response delay.\nThree resource-stable trials establish an observed workload boundary, not a universal safe maximum.\n'+f"Minimum selected cgroup headroom: {1024-edge['stable_cgroup_peak_mib']:.2f} MiB ({edge['body_mib']} MiB JSON).",fontsize=8,color='#555')
fig.subplots_adjust(left=.09,right=.98,top=.86,bottom=.12,hspace=.62)
args.output.parent.mkdir(parents=True,exist_ok=True)
fig.savefig(args.output,facecolor='white')
