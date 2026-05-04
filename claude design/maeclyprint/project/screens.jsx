// Print Job, Queue, Profiles, Settings, About screens

// ─────────── PRINT JOB ───────────
function ScreenPrintJob({ files, setFiles, profile, setProfile, overrides, setOverrides, density='comfortable', onPrint, layout='split' }) {
  const profileObj = MP_PROFILES.find(p=>p.id===profile) || MP_PROFILES[0];
  const merged = { ...profileObj, ...overrides };
  const totalPages = files.reduce((s,f)=>s+(f.pages||1), 0);
  const totalKb = files.reduce((s,f)=>s+f.kb, 0);

  const remove = (id) => setFiles(files.filter(f=>f.id!==id));
  const move = (id, dir) => {
    const i = files.findIndex(f=>f.id===id); const j = i+dir;
    if (i<0||j<0||j>=files.length) return;
    const next = files.slice(); [next[i], next[j]] = [next[j], next[i]];
    setFiles(next);
  };

  return (
    <>
      <MPTopBar title="Print Job" subtitle={`${files.length} file${files.length===1?'':'s'} · ${totalPages} pages · ${fmtKb(totalKb)}`} right={
        <>
          <MPButton variant="ghost" icon="plus" size="sm">Add files</MPButton>
          <MPButton variant="primary" icon="printer" onClick={onPrint}>Print {files.length}</MPButton>
        </>
      }/>
      <div style={{flex:1, display:'flex', minHeight:0}}>
        {/* File list */}
        <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:'14px 16px', minWidth:0}}>
          <div style={{display:'flex', justifyContent:'space-between', alignItems:'center', marginBottom:8}}>
            <MPSectionLabel>Files in this job</MPSectionLabel>
            <div style={{display:'flex', gap:6, alignItems:'center'}}>
              <MPKbd>↑↓</MPKbd>
              <span style={{fontSize:10.5, color:'var(--mp-textDim)'}}>reorder</span>
            </div>
          </div>
          <MPCard padding={0}>
            {files.map((f,i) => (
              <div key={f.id} style={{
                display:'flex', alignItems:'center', gap:10,
                padding: density==='compact' ? '6px 12px' : '10px 12px',
                borderBottom: i<files.length-1 ? '1px solid var(--mp-border)' : 'none',
              }}>
                <span className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', width:18, textAlign:'right'}}>{(i+1).toString().padStart(2,'0')}</span>
                <FileGlyph ext={f.ext} size={density==='compact'?18:24}/>
                <div style={{flex:1, minWidth:0}}>
                  <div style={{fontSize:12.5, fontWeight:500, overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>{f.name}.{f.ext}</div>
                  {density!=='compact' && (
                    <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', marginTop:1}}>
                      {f.pages ? `${f.pages} page${f.pages===1?'':'s'}` : 'image'} · {fmtKb(f.kb)}
                    </div>
                  )}
                </div>
                <MPIconButton icon="chevu" size={22} label="Move up" onClick={()=>move(f.id,-1)}/>
                <MPIconButton icon="chevd" size={22} label="Move down" onClick={()=>move(f.id,+1)}/>
                <MPIconButton icon="x" size={22} label="Remove" onClick={()=>remove(f.id)}/>
              </div>
            ))}
            {files.length===0 && (
              <div style={{padding:'30px 16px', textAlign:'center', color:'var(--mp-textMuted)', fontSize:12.5}}>
                No files. Drag from Explorer or click <strong>Add files</strong>.
              </div>
            )}
          </MPCard>
        </div>

        {/* Settings panel */}
        <div className="mp-scroll" style={{
          width: 280, flexShrink:0, overflow:'auto',
          background:'var(--mp-surface2)', borderLeft:'1px solid var(--mp-border)',
          padding:'14px 16px',
        }}>
          <MPSectionLabel>Profile</MPSectionLabel>
          <MPSelect fullWidth value={profile} onChange={e=>setProfile(e.target.value)}
            options={MP_PROFILES.map(p=>({value:p.id, label:p.name + (p.builtin?' · built-in':'')}))}/>
          <div style={{height:14}}/>

          <MPSectionLabel>Printer</MPSectionLabel>
          <MPSelect fullWidth value={overrides.printer || merged.printer} onChange={e=>setOverrides({...overrides, printer:e.target.value})}
            options={[{value:'<default>', label:'System default'}, ...MP_PRINTERS.map(p=>({value:p.name, label:p.name}))]}/>
          <div style={{height:14}}/>

          <MPSectionLabel>Sides</MPSectionLabel>
          <MPSegmented value={merged.duplex} onChange={v=>setOverrides({...overrides, duplex:v})}
            options={[{value:'off', label:'Single'},{value:'long-edge', label:'Long'},{value:'short-edge', label:'Short'}]}/>
          <div style={{height:14}}/>

          <MPSectionLabel>Color</MPSectionLabel>
          <MPSegmented value={merged.color} onChange={v=>setOverrides({...overrides, color:v})}
            options={[{value:'color', label:'Color'},{value:'monochrome', label:'B&W'}]}/>
          <div style={{height:14}}/>

          <MPSectionLabel>Copies</MPSectionLabel>
          <MPInput fullWidth mono value={String(merged.copies)} onChange={e=>setOverrides({...overrides, copies:parseInt(e.target.value)||1})} type="number" suffix="× per file"/>
          <div style={{height:14}}/>

          <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:10}}>
            <div>
              <MPSectionLabel>Paper</MPSectionLabel>
              <MPSelect fullWidth value={merged.paper} onChange={e=>setOverrides({...overrides, paper:e.target.value})}
                options={['A4','Letter','Legal','A3'].map(v=>({value:v,label:v}))}/>
            </div>
            <div>
              <MPSectionLabel>Orient.</MPSectionLabel>
              <MPSelect fullWidth value={merged.orientation} onChange={e=>setOverrides({...overrides, orientation:e.target.value})}
                options={[{value:'auto',label:'Auto'},{value:'portrait',label:'Portrait'},{value:'landscape',label:'Landscape'}]}/>
            </div>
          </div>
          <div style={{height:14}}/>

          <MPSectionLabel>Scaling</MPSectionLabel>
          <MPSegmented value={merged.scaling} onChange={v=>setOverrides({...overrides, scaling:v})}
            options={[{value:'fit',label:'Fit'},{value:'shrink',label:'Shrink'},{value:'none',label:'100%'}]}/>
          <div style={{height:14}}/>

          <MPSectionLabel>Page range (PDFs)</MPSectionLabel>
          <MPInput fullWidth mono placeholder="e.g. 1-5,10" value={overrides.range||''} onChange={e=>setOverrides({...overrides, range:e.target.value})}/>

          <div style={{height:18}}/>
          <div style={{padding:'10px 12px', background:'var(--mp-surface)', border:'1px solid var(--mp-border)', borderRadius:6}}>
            <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', marginBottom:6, letterSpacing:'0.06em'}}>RESOLVED COMMAND</div>
            <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textMuted)', lineHeight:1.5, wordBreak:'break-all'}}>
              SumatraPDF -print-to "{merged.printer==='<default>'?'<default>':merged.printer}" -print-settings "{merged.duplex==='long-edge'?'duplex,':merged.duplex==='short-edge'?'duplexshort,':''}{merged.color==='monochrome'?'monochrome,':'color,'}{merged.scaling},paper={merged.paper},copies={merged.copies}" -silent
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

// ─────────── QUEUE ───────────
function ScreenQueue({ jobs, setJobs, view='list' }) {
  const groups = {
    active: jobs.filter(j=>['printing','converting','converted'].includes(j.status)),
    pending: jobs.filter(j=>['queued','paused'].includes(j.status)),
    done: jobs.filter(j=>['done','failed','cancelled'].includes(j.status)),
  };
  const toggleJob = (id, st) => setJobs(jobs.map(j=>j.id===id?{...j, status:st}:j));

  if (view==='board') return <QueueBoard jobs={jobs}/>;
  if (view==='timeline') return <QueueTimeline jobs={jobs}/>;

  const Row = ({ j }) => (
    <div style={{
      display:'grid', gridTemplateColumns:'24px 1fr 110px 90px 110px 32px',
      gap:10, alignItems:'center', padding:'8px 14px',
      borderBottom:'1px solid var(--mp-border)',
    }}>
      <FileGlyph ext={j.ext} size={20}/>
      <div style={{minWidth:0}}>
        <div style={{fontSize:12.5, fontWeight:500, overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>{j.file}</div>
        <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', marginTop:1, overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>
          {j.profile} · {j.printer}{j.note?` · ${j.note}`:''}{j.error?` · ${j.error}`:''}
        </div>
      </div>
      <div style={{display:'flex', alignItems:'center', gap:6}}>
        <StatusDot status={j.status}/>
        <span className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textMuted)', textTransform:'uppercase'}}>{j.status}</span>
      </div>
      <div style={{height:5, background:'var(--mp-surface3)', borderRadius:3, overflow:'hidden', position:'relative'}}>
        <div style={{
          position:'absolute', inset:0,
          width: `${(j.progress||0)*100}%`,
          background: j.status==='failed'?'var(--mp-error)':j.status==='done'?'var(--mp-success)':'var(--mp-accent)',
          transition:'width .3s',
        }}/>
      </div>
      <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textMuted)', textAlign:'right'}}>
        {j.startedAt}{j.finishedAt?` → ${j.finishedAt}`:''}
      </div>
      <div style={{display:'flex'}}>
        {j.status==='printing'||j.status==='converting' ? <MPIconButton icon="pause" size={24} label="Pause" onClick={()=>toggleJob(j.id,'paused')}/> :
         j.status==='paused'||j.status==='queued' ? <MPIconButton icon="play" size={24} label="Resume" onClick={()=>toggleJob(j.id,'printing')}/> :
         j.status==='failed' ? <MPIconButton icon="rotate" size={24} label="Retry" onClick={()=>toggleJob(j.id,'queued')}/> :
         <MPIconButton icon="more" size={24} label="More"/>}
      </div>
    </div>
  );

  return (
    <>
      <MPTopBar title="Queue" subtitle={`${groups.active.length} active · ${groups.pending.length} pending · ${groups.done.length} done`} right={
        <>
          <MPSegmented value={view} onChange={()=>{}} options={[
            {value:'list',icon:'list',label:'List'},
            {value:'board',icon:'grid',label:'Board'},
            {value:'timeline',icon:'queue',label:'Timeline'},
          ]} size="sm"/>
          <MPButton variant="ghost" icon="pause" size="sm">Pause queue</MPButton>
          <MPButton variant="ghost" icon="trash" size="sm">Clear done</MPButton>
        </>
      }/>
      <div className="mp-scroll" style={{flex:1, overflow:'auto'}}>
        {[
          {key:'active', label:'Active', items:groups.active},
          {key:'pending', label:'Pending', items:groups.pending},
          {key:'done', label:'Recent', items:groups.done},
        ].map(g => g.items.length>0 && (
          <div key={g.key}>
            <div style={{
              padding:'10px 14px', background:'var(--mp-surface2)',
              borderBottom:'1px solid var(--mp-border)', borderTop: g.key!=='active'?'1px solid var(--mp-border)':'none',
              display:'flex', justifyContent:'space-between', alignItems:'center',
            }}>
              <span className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textMuted)', textTransform:'uppercase', letterSpacing:'0.08em', fontWeight:500}}>
                {g.label} · {g.items.length}
              </span>
            </div>
            {g.items.map(j => <Row key={j.id} j={j}/>)}
          </div>
        ))}
      </div>
    </>
  );
}

function QueueBoard({ jobs }) {
  const cols = [
    { key:'queued', label:'Queued', items: jobs.filter(j=>j.status==='queued'||j.status==='paused') },
    { key:'converting', label:'Converting', items: jobs.filter(j=>j.status==='converting') },
    { key:'printing', label:'Printing', items: jobs.filter(j=>j.status==='printing') },
    { key:'done', label:'Done', items: jobs.filter(j=>['done','failed'].includes(j.status)) },
  ];
  return (
    <>
      <MPTopBar title="Queue" subtitle="Board view — kanban across job phases" right={
        <MPSegmented value="board" onChange={()=>{}} size="sm" options={[
          {value:'list',icon:'list',label:'List'},
          {value:'board',icon:'grid',label:'Board'},
          {value:'timeline',icon:'queue',label:'Timeline'},
        ]}/>
      }/>
      <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:14, display:'grid', gridTemplateColumns:'repeat(4,1fr)', gap:12}}>
        {cols.map(c => (
          <div key={c.key} style={{display:'flex', flexDirection:'column', gap:8}}>
            <div style={{display:'flex', justifyContent:'space-between', padding:'2px 4px'}}>
              <span className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textMuted)', textTransform:'uppercase', letterSpacing:'0.08em'}}>{c.label}</span>
              <span className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)'}}>{c.items.length}</span>
            </div>
            {c.items.map(j => (
              <MPCard key={j.id} padding={10} hoverable>
                <div style={{display:'flex', alignItems:'center', gap:8, marginBottom:6}}>
                  <FileGlyph ext={j.ext} size={18}/>
                  <span style={{fontSize:11.5, fontWeight:500, overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap', flex:1, minWidth:0}}>{j.file}</span>
                </div>
                <div className="mp-mono" style={{fontSize:10, color:'var(--mp-textDim)', marginBottom:8}}>
                  {j.pages}p · {j.profile}
                </div>
                <div style={{display:'flex', alignItems:'center', gap:6}}>
                  <StatusDot status={j.status}/>
                  <div style={{flex:1, height:3, background:'var(--mp-surface3)', borderRadius:2, overflow:'hidden'}}>
                    <div style={{height:'100%', width:`${(j.progress||0)*100}%`, background:'var(--mp-accent)'}}/>
                  </div>
                </div>
              </MPCard>
            ))}
          </div>
        ))}
      </div>
    </>
  );
}

function QueueTimeline({ jobs }) {
  return (
    <>
      <MPTopBar title="Queue" subtitle="Timeline — chronological, with durations" right={
        <MPSegmented value="timeline" onChange={()=>{}} size="sm" options={[
          {value:'list',icon:'list',label:'List'},
          {value:'board',icon:'grid',label:'Board'},
          {value:'timeline',icon:'queue',label:'Timeline'},
        ]}/>
      }/>
      <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:'14px 20px'}}>
        {jobs.map((j,i) => (
          <div key={j.id} style={{display:'flex', gap:12, paddingBottom:14, position:'relative'}}>
            <div style={{display:'flex', flexDirection:'column', alignItems:'center', flexShrink:0, paddingTop:4}}>
              <StatusDot status={j.status}/>
              {i<jobs.length-1 && <div style={{flex:1, width:1, background:'var(--mp-border)', marginTop:4}}/>}
            </div>
            <div style={{flex:1, minWidth:0, paddingBottom:6}}>
              <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', marginBottom:2}}>
                {j.startedAt}{j.finishedAt?` → ${j.finishedAt}`:''}
              </div>
              <div style={{display:'flex', alignItems:'center', gap:8}}>
                <FileGlyph ext={j.ext} size={18}/>
                <span style={{fontSize:12.5, fontWeight:500}}>{j.file}</span>
                <MPBadge tone={j.status==='done'?'success':j.status==='failed'?'error':j.status==='printing'?'accent':'neutral'}>{j.status}</MPBadge>
              </div>
              <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textMuted)', marginTop:4}}>
                {j.printer} · {j.profile} · {j.pages}p{j.error?` · ${j.error}`:''}
              </div>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

// ─────────── PROFILES ───────────
function ScreenProfiles({ selectedId, onSelect }) {
  const sel = MP_PROFILES.find(p=>p.id===selectedId) || MP_PROFILES[0];
  return (
    <>
      <MPTopBar title="Profiles" subtitle="Saved combinations of printer + settings" right={
        <MPButton variant="primary" icon="plus">New profile</MPButton>
      }/>
      <div style={{flex:1, display:'flex', minHeight:0}}>
        <div className="mp-scroll" style={{width:280, flexShrink:0, overflow:'auto', borderRight:'1px solid var(--mp-border)', padding:'10px 8px', background:'var(--mp-surface2)'}}>
          {MP_PROFILES.map(p => {
            const a = p.id===sel.id;
            return (
              <button key={p.id} onClick={()=>onSelect?.(p.id)} className="mp-focus" style={{
                width:'100%', display:'flex', alignItems:'center', gap:10,
                padding:'9px 10px', border:'none', background: a?'var(--mp-surface)':'transparent',
                borderRadius:6, cursor:'pointer', textAlign:'left', marginBottom:2,
                boxShadow: a ? '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px var(--mp-border)' : 'none',
              }}>
                <div style={{
                  width:28, height:28, borderRadius:6, background: p.builtin?'var(--mp-surface2)':'var(--mp-accentSoft)',
                  display:'flex', alignItems:'center', justifyContent:'center', flexShrink:0,
                }}>
                  <Icon name={p.builtin?'lock':'sliders'} size={14} color={p.builtin?'var(--mp-textMuted)':'var(--mp-accent)'}/>
                </div>
                <div style={{flex:1, minWidth:0}}>
                  <div style={{display:'flex', alignItems:'center', gap:6}}>
                    <span style={{fontSize:12.5, fontWeight:500}}>{p.name}</span>
                    {p.default && <MPBadge tone="accent">default</MPBadge>}
                  </div>
                  <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>
                    {p.color} · {p.duplex==='off'?'simplex':p.duplex} · {p.paper}
                  </div>
                </div>
              </button>
            );
          })}
        </div>
        <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:'18px 22px'}}>
          <div style={{display:'flex', alignItems:'center', justifyContent:'space-between', marginBottom:18}}>
            <div>
              <div style={{display:'flex', alignItems:'center', gap:10}}>
                <h2 style={{margin:0, fontSize:18, fontWeight:600, letterSpacing:'-0.01em'}}>{sel.name}</h2>
                {sel.builtin && <MPBadge tone="neutral" icon="lock">built-in</MPBadge>}
                {sel.default && <MPBadge tone="accent">default</MPBadge>}
              </div>
              <div className="mp-mono" style={{fontSize:11, color:'var(--mp-textDim)', marginTop:4}}>id: {sel.id} · schema_version: 1</div>
            </div>
            <div style={{display:'flex', gap:6}}>
              <MPButton variant="ghost" icon="copy" size="sm">Duplicate</MPButton>
              <MPButton variant="ghost" icon="star" size="sm" disabled={sel.default}>Make default</MPButton>
              {!sel.builtin && <MPButton variant="danger" icon="trash" size="sm">Delete</MPButton>}
            </div>
          </div>

          {[
            { label:'Printer',     v: sel.printer, mono:true },
            { label:'Color mode',  v: sel.color },
            { label:'Duplex',      v: sel.duplex==='off'?'Single-sided':sel.duplex },
            { label:'Copies',      v: sel.copies, mono:true },
            { label:'Paper',       v: sel.paper, mono:true },
            { label:'Orientation', v: sel.orientation },
            { label:'Scaling',     v: sel.scaling },
            { label:'Quality',     v: sel.quality },
            { label:'Tray',        v: sel.tray },
          ].map((row,i)=>(
            <div key={i} style={{
              display:'grid', gridTemplateColumns:'140px 1fr', gap:14,
              padding:'10px 0', borderBottom:'1px solid var(--mp-border)',
            }}>
              <span style={{fontSize:12, color:'var(--mp-textMuted)'}}>{row.label}</span>
              <span className={row.mono?'mp-mono':''} style={{fontSize:13, color:'var(--mp-text)'}}>{row.v}</span>
            </div>
          ))}

          <div style={{marginTop:20}}>
            <MPSectionLabel>profiles.toml entry</MPSectionLabel>
            <pre className="mp-mono" style={{
              margin:0, padding:'12px 14px', background:'var(--mp-surface2)',
              border:'1px solid var(--mp-border)', borderRadius:6,
              fontSize:11, lineHeight:1.6, color:'var(--mp-textMuted)', overflow:'auto',
            }}>{`[[profile]]
id          = "${sel.id}"
name        = "${sel.name}"
printer     = "${sel.printer}"
color_mode  = "${sel.color}"
duplex      = "${sel.duplex}"
copies      = ${sel.copies}
paper_size  = "${sel.paper}"
orientation = "${sel.orientation}"
scaling     = "${sel.scaling}"
quality     = "${sel.quality}"
tray        = "${sel.tray}"`}</pre>
          </div>
        </div>
      </div>
    </>
  );
}

// ─────────── SETTINGS ───────────
function ScreenSettings({ theme, setTheme, density, setDensity, shellExt, setShellExt, channel, setChannel }) {
  const Row = ({ label, hint, children }) => (
    <div style={{
      display:'grid', gridTemplateColumns:'1fr auto', gap:20,
      padding:'14px 0', borderBottom:'1px solid var(--mp-border)', alignItems:'center',
    }}>
      <div>
        <div style={{fontSize:13, fontWeight:500}}>{label}</div>
        {hint && <div style={{fontSize:11.5, color:'var(--mp-textMuted)', marginTop:2}}>{hint}</div>}
      </div>
      <div>{children}</div>
    </div>
  );
  return (
    <>
      <MPTopBar title="Settings"/>
      <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:'18px 24px', maxWidth:760}}>
        <MPSectionLabel>Appearance</MPSectionLabel>
        <Row label="Theme" hint="Follows OS by default.">
          <MPSegmented value={theme} onChange={setTheme} options={[
            {value:'light', icon:'sun', label:'Light'},
            {value:'dark', icon:'moon', label:'Dark'},
          ]}/>
        </Row>
        <Row label="Density" hint="Comfortable for reading, compact for big batches.">
          <MPSegmented value={density} onChange={setDensity} options={[
            {value:'comfortable', label:'Comfortable'},
            {value:'compact', label:'Compact'},
          ]}/>
        </Row>

        <div style={{height:24}}/>
        <MPSectionLabel>Shell integration</MPSectionLabel>
        <Row label="Right-click menu" hint="Adds 'Print with MaeclyPrint' to Windows 11 modern menu.">
          <MPSwitch checked={shellExt} onChange={()=>setShellExt(!shellExt)}/>
        </Row>
        <Row label="Filter unsupported files" hint="Hide menu when no printable files are selected.">
          <MPSwitch checked={true} onChange={()=>{}}/>
        </Row>
        <Row label="Default profile" hint="Used when invoking from context menu directly.">
          <MPSelect value="p2" options={MP_PROFILES.map(p=>({value:p.id,label:p.name}))}/>
        </Row>

        <div style={{height:24}}/>
        <MPSectionLabel>Engine</MPSectionLabel>
        <Row label="Concurrent jobs" hint="Higher values can saturate the printer queue.">
          <MPInput mono value="2" suffix="jobs"/>
        </Row>
        <Row label="Conversion timeout" hint="Per-file LibreOffice/Chromium conversion budget.">
          <MPInput mono value="60" suffix="seconds"/>
        </Row>
        <Row label="Max file size" hint="Files larger than this prompt for confirmation.">
          <MPInput mono value="200" suffix="MiB"/>
        </Row>

        <div style={{height:24}}/>
        <MPSectionLabel>Updates</MPSectionLabel>
        <Row label="Channel">
          <MPSegmented value={channel} onChange={setChannel} options={[
            {value:'stable',label:'Stable'},{value:'beta',label:'Beta'},{value:'nightly',label:'Nightly'},
          ]}/>
        </Row>
        <Row label="Check on startup" hint="Delayed by 30 seconds to keep launch fast.">
          <MPSwitch checked={true} onChange={()=>{}}/>
        </Row>

        <div style={{height:24}}/>
        <MPSectionLabel>Privacy</MPSectionLabel>
        <Row label="Crash reports" hint="Off by default. Sent only when toggled on.">
          <MPSwitch checked={false} onChange={()=>{}}/>
        </Row>
        <Row label="Anonymous usage metrics" hint="MaeclyPrint never enables this without consent.">
          <MPSwitch checked={false} onChange={()=>{}}/>
        </Row>
      </div>
    </>
  );
}

// ─────────── ABOUT ───────────
function ScreenAbout() {
  const deps = [
    { name:'Tauri', v:'2.1.0', license:'MIT/Apache-2.0' },
    { name:'SumatraPDF', v:'3.5.2', license:'GPLv3' },
    { name:'LibreOffice', v:'auto-detect', license:'MPL-2.0' },
    { name:'ImageMagick', v:'7.1.x', license:'ImageMagick' },
    { name:'Pandoc', v:'auto-detect', license:'GPLv2+' },
    { name:'qpdf', v:'11.x', license:'Apache-2.0' },
  ];
  return (
    <>
      <MPTopBar title="About"/>
      <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:'24px 28px'}}>
        <div style={{display:'flex', alignItems:'center', gap:16, marginBottom:18}}>
          <MPAppIcon size={56}/>
          <div>
            <div style={{fontSize:18, fontWeight:600, letterSpacing:'-0.01em'}}>MaeclyPrint</div>
            <div className="mp-mono" style={{fontSize:11.5, color:'var(--mp-textMuted)', marginTop:2}}>
              0.1.0-alpha · build 2026.05.03 · stable channel
            </div>
            <div style={{fontSize:12, color:'var(--mp-textMuted)', marginTop:6, maxWidth:420}}>
              Open source batch printing for Windows and macOS. Right-click → print everything.
            </div>
          </div>
        </div>

        <div style={{display:'flex', gap:8, marginBottom:24}}>
          <MPButton variant="primary" icon="rotate" size="sm">Check for updates</MPButton>
          <MPButton variant="ghost" icon="page" size="sm">Release notes</MPButton>
          <MPButton variant="ghost" icon="shield" size="sm">License</MPButton>
        </div>

        <MPSectionLabel>Up to date</MPSectionLabel>
        <MPCard padding={14} style={{marginBottom:24}}>
          <div style={{display:'flex', alignItems:'center', gap:10}}>
            <div style={{width:32, height:32, borderRadius:'50%', background:'var(--mp-successSoft)', display:'flex', alignItems:'center', justifyContent:'center'}}>
              <Icon name="check" size={16} color="var(--mp-success)"/>
            </div>
            <div style={{flex:1}}>
              <div style={{fontSize:13, fontWeight:500}}>You're running the latest version.</div>
              <div className="mp-mono" style={{fontSize:11, color:'var(--mp-textDim)', marginTop:2}}>last checked 12 minutes ago</div>
            </div>
          </div>
        </MPCard>

        <MPSectionLabel>Bundled dependencies</MPSectionLabel>
        <MPCard padding={0}>
          {deps.map((d,i) => (
            <div key={d.name} style={{
              display:'grid', gridTemplateColumns:'1fr 100px 140px',
              padding:'10px 14px', gap:12,
              borderBottom: i<deps.length-1 ? '1px solid var(--mp-border)':'none',
              fontSize:12.5,
            }}>
              <span style={{fontWeight:500}}>{d.name}</span>
              <span className="mp-mono" style={{color:'var(--mp-textMuted)'}}>{d.v}</span>
              <span className="mp-mono" style={{color:'var(--mp-textDim)'}}>{d.license}</span>
            </div>
          ))}
        </MPCard>

        <div style={{marginTop:24, fontSize:11.5, color:'var(--mp-textDim)', lineHeight:1.7}}>
          MaeclyPrint is free software licensed under <span className="mp-mono">GPLv3</span>. No telemetry, no cloud, no account.
          Source code at <span className="mp-mono">github.com/maecly/maeclyprint</span>.
        </div>
      </div>
    </>
  );
}

Object.assign(window, { ScreenPrintJob, ScreenQueue, QueueBoard, QueueTimeline, ScreenProfiles, ScreenSettings, ScreenAbout });
