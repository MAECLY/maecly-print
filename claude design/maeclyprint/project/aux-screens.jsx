// MaeclyPrint — auxiliary screens: context menu, onboarding, toasts, updates, macOS

// ─────────── WIN11 CONTEXT MENU ───────────
function Win11Context({ variant='modern', selectedFiles=4, submenu='flat' }) {
  const W = 280;
  if (variant==='classic') {
    return (
      <div style={{
        width: W, background:'#F5F5F5', border:'1px solid #ACA899',
        boxShadow:'2px 2px 0 rgba(0,0,0,0.15)', padding:'2px',
        fontFamily:'"Segoe UI","Tahoma",sans-serif', fontSize:11.5, color:'#000',
      }}>
        {[
          {ico:'📄', label:'Open'},
          {ico:'✏️', label:'Edit'},
          {ico:'🖨', label:'Print'},
          {ico:null, sep:true},
          {ico:null, label:'Print with MaeclyPrint', accent:true, sub:true},
          {ico:null, sep:true},
          {ico:null, label:'7-Zip', sub:true},
          {ico:null, label:'Send to', sub:true},
          {ico:null, sep:true},
          {ico:null, label:'Cut'},
          {ico:null, label:'Copy'},
          {ico:null, sep:true},
          {ico:null, label:'Properties'},
        ].map((it,i) => it.sep ? (
          <div key={i} style={{height:1, background:'#D4D0C8', margin:'2px 1px', borderTop:'1px solid #FFFFFF'}}/>
        ) : (
          <div key={i} style={{
            display:'flex', alignItems:'center', gap:8, padding:'4px 22px 4px 28px',
            background: it.accent?'#316AC5':'transparent', color: it.accent?'#FFF':'#000',
            position:'relative',
          }}>
            {it.accent && <div style={{position:'absolute', left:5, top:5, bottom:5, width:18, background:'#FFF', display:'flex', alignItems:'center', justifyContent:'center', border:'1px solid rgba(0,0,0,0.1)'}}>
              <MPGlyph size={11}/>
            </div>}
            <span>{it.label}</span>
            {it.sub && <span style={{marginLeft:'auto', fontSize:10}}>▶</span>}
          </div>
        ))}
      </div>
    );
  }

  // Modern Win11 menu
  const items = submenu==='flat' ? [
    { sect: 'top', items: [
      { ico:'cut', label:'Cut', kbd:'Ctrl+X' },
      { ico:'copy', label:'Copy', kbd:'Ctrl+C' },
      { ico:'rename', label:'Rename', kbd:'F2' },
      { ico:'share', label:'Share' },
      { ico:'trash', label:'Delete', kbd:'Del' },
    ]},
    { sect: 'mp', items: [
      { ico:'mp', label:`Print with MaeclyPrint (${selectedFiles})`, accent:true, kbd:'⊞+P' },
    ]},
    { sect: 'rest', items: [
      { ico:'open', label:'Open', kbd:'Enter' },
      { ico:'openwith', label:'Open with', sub:true },
      { ico:'compress', label:'Compress to ZIP file' },
      { ico:'page', label:'Properties', kbd:'Alt+Enter' },
      { ico:'more', label:'Show more options', kbd:'Shift+F10', dim:true },
    ]},
  ] : submenu==='nested' ? [
    { sect: 'top', items: [
      { ico:'cut', label:'Cut', kbd:'Ctrl+X' },
      { ico:'copy', label:'Copy', kbd:'Ctrl+C' },
      { ico:'share', label:'Share' },
    ]},
    { sect: 'mp', items: [
      { ico:'mp', label:'Print with MaeclyPrint', accent:true, sub:true },
    ]},
    { sect: 'rest', items: [
      { ico:'open', label:'Open' },
      { ico:'page', label:'Properties' },
      { ico:'more', label:'Show more options', dim:true },
    ]},
  ] : [
    { sect: 'top', items: [
      { ico:'cut', label:'Cut', kbd:'Ctrl+X' },
      { ico:'copy', label:'Copy', kbd:'Ctrl+C' },
      { ico:'share', label:'Share' },
      { ico:'trash', label:'Delete' },
    ]},
    { sect: 'mp', items: [
      { ico:'mp-print', label:`Quick print · ${selectedFiles} files`, accent:true },
      { ico:'mp-prof', label:'Print with profile…', accent:true, sub:true },
      { ico:'mp-q', label:'Send to MaeclyPrint queue', accent:true },
    ]},
    { sect: 'rest', items: [
      { ico:'open', label:'Open' },
      { ico:'more', label:'Show more options', dim:true },
    ]},
  ];

  return (
    <div style={{
      width: W, background:'rgba(249,249,249,0.92)', backdropFilter:'blur(40px)',
      border:'1px solid rgba(0,0,0,0.08)', borderRadius:8,
      boxShadow:'0 8px 24px rgba(0,0,0,0.18), 0 0 0 1px rgba(255,255,255,0.6) inset',
      padding:'4px', fontFamily:'"Segoe UI Variable","Segoe UI",sans-serif', fontSize:12.5, color:'#1A1814',
    }}>
      {items.map((sect, si) => (
        <React.Fragment key={si}>
          {si>0 && <div style={{height:1, background:'rgba(0,0,0,0.08)', margin:'4px 6px'}}/>}
          {sect.items.map((it,i) => (
            <div key={i} style={{
              display:'flex', alignItems:'center', gap:10, padding:'6px 10px',
              borderRadius:4, color: it.dim ? 'rgba(0,0,0,0.5)' : '#1A1814',
              background: it.accent && sect.sect==='mp' && i===0 ? 'rgba(194,65,12,0.08)' : 'transparent',
              position:'relative',
            }}>
              <span style={{width:16, height:16, display:'flex', alignItems:'center', justifyContent:'center', flexShrink:0}}>
                {it.ico==='mp' || it.ico==='mp-print' || it.ico==='mp-prof' || it.ico==='mp-q' ? <MPGlyph size={14}/> :
                 <Icon name={ctxIconMap(it.ico)} size={14} color={it.dim?'rgba(0,0,0,0.5)':'#3A3530'}/>}
              </span>
              <span style={{flex:1, fontWeight: it.accent?500:400}}>{it.label}</span>
              {it.kbd && <span className="mp-mono" style={{fontSize:10.5, color:'rgba(0,0,0,0.45)'}}>{it.kbd}</span>}
              {it.sub && <Icon name="chevr" size={12} color="rgba(0,0,0,0.5)"/>}
            </div>
          ))}
        </React.Fragment>
      ))}
    </div>
  );
}
function ctxIconMap(k) {
  return ({cut:'cut', copy:'copy', rename:'rename', share:'share', trash:'trash',
    open:'page', openwith:'openwith', compress:'archive', page:'page', more:'dots',
  })[k] || 'page';
}

// Context menu shown in-context against an Explorer-ish file row backdrop
function ContextMenuStage({ variant, submenu, selectedFiles=4 }) {
  return (
    <div style={{
      width:'100%', height:'100%', background:'#F3F3F3', overflow:'hidden',
      fontFamily:'"Segoe UI Variable","Segoe UI",sans-serif', position:'relative',
    }}>
      {/* Faux explorer toolbar */}
      <div style={{height:40, background:'#FFF', borderBottom:'1px solid #E5E5E5', display:'flex', alignItems:'center', gap:8, padding:'0 12px'}}>
        <div style={{display:'flex', gap:4}}>
          {['arrowl','arrowr','rotate','arrowu'].map(i => <div key={i} style={{width:24,height:24,display:'flex',alignItems:'center',justifyContent:'center',color:'#5A5A5A'}}><Icon name={i} size={14}/></div>)}
        </div>
        <div style={{flex:1, height:24, background:'#F5F5F5', borderRadius:4, padding:'0 8px', display:'flex', alignItems:'center', gap:6, fontSize:11.5, color:'#555'}}>
          <Icon name="folder" size={12} color="#C2A14A"/>
          <span>This PC › Documents › Reports</span>
        </div>
      </div>
      {/* Faux file list */}
      <div style={{padding:'8px 0'}}>
        {[
          {n:'Q4-financial-summary.docx', ext:'docx', sel:true},
          {n:'budget-2026.xlsx', ext:'xlsx', sel:true},
          {n:'team-roster.pdf', ext:'pdf', sel:true},
          {n:'roadmap-deck.pptx', ext:'pptx', sel:true},
          {n:'meeting-notes.md', ext:'md', sel:false},
        ].map((f,i) => (
          <div key={i} style={{
            display:'flex', alignItems:'center', gap:10, padding:'6px 14px', fontSize:12,
            background: f.sel ? 'rgba(0,120,212,0.10)' : 'transparent',
            border: f.sel ? '1px solid rgba(0,120,212,0.20)' : '1px solid transparent',
            margin:'1px 6px', borderRadius:4,
          }}>
            <FileGlyph ext={f.ext} size={20}/>
            <span style={{flex:1, color:'#1A1A1A'}}>{f.n}</span>
            <span className="mp-mono" style={{fontSize:10.5, color:'#888'}}>2026-04-{20+i}</span>
          </div>
        ))}
      </div>
      {/* Menu floating over */}
      <div style={{position:'absolute', left:120, top:90}}>
        <Win11Context variant={variant} submenu={submenu} selectedFiles={selectedFiles}/>
      </div>
    </div>
  );
}

// ─────────── ONBOARDING ───────────
function ScreenOnboarding({ step=0 }) {
  const steps = [
    { title:'Welcome to MaeclyPrint', sub:'Print everything by right-click. No 15-file limits, no Adobe.' },
    { title:'Find your printers',     sub:'We saw your printers. Pick a default — you can change it any time.' },
    { title:'Right-click integration',sub:'Add MaeclyPrint to the Windows 11 context menu? Recommended.' },
    { title:'Make your first profile',sub:'Profiles save common combinations. Start with the default one.' },
    { title:'You\'re ready',           sub:'Drag files into MaeclyPrint, or right-click them in Explorer.' },
  ];
  const s = steps[step];
  return (
    <div style={{flex:1, display:'flex', flexDirection:'column', background:'var(--mp-bg)'}}>
      {/* Top: progress dots */}
      <div style={{padding:'14px 18px', display:'flex', justifyContent:'space-between', alignItems:'center'}}>
        <MPWordmark size={12}/>
        <div style={{display:'flex', gap:6}}>
          {steps.map((_,i)=>(
            <div key={i} style={{
              width: i===step?20:6, height:6, borderRadius:3,
              background: i<=step?'var(--mp-accent)':'var(--mp-surface3)',
              transition:'width .25s, background .25s',
            }}/>
          ))}
        </div>
        <button className="mp-focus" style={{background:'none',border:'none',cursor:'pointer',fontSize:11.5,color:'var(--mp-textMuted)'}}>Skip</button>
      </div>

      {/* Body */}
      <div style={{flex:1, display:'flex', flexDirection:'column', justifyContent:'center', padding:'0 40px'}}>
        <div style={{maxWidth:440}}>
          <div style={{marginBottom:18}}>
            <MPAppIcon size={48}/>
          </div>
          <h1 style={{fontSize:24, fontWeight:600, letterSpacing:'-0.02em', margin:'0 0 8px'}}>{s.title}</h1>
          <p style={{fontSize:13.5, color:'var(--mp-textMuted)', margin:'0 0 22px', lineHeight:1.55}}>{s.sub}</p>

          {step===1 && (
            <div style={{display:'flex', flexDirection:'column', gap:6}}>
              {MP_PRINTERS.map((p,i)=>(
                <label key={p.name} style={{display:'flex', alignItems:'center', gap:10, padding:'10px 12px', background:'var(--mp-surface)', border:`1px solid ${i===0?'var(--mp-accent)':'var(--mp-border)'}`, borderRadius:6, cursor:'pointer'}}>
                  <span style={{width:14,height:14,borderRadius:'50%',border:`1.5px solid ${i===0?'var(--mp-accent)':'var(--mp-borderStrong)'}`,display:'flex',alignItems:'center',justifyContent:'center'}}>
                    {i===0 && <span style={{width:6,height:6,borderRadius:'50%',background:'var(--mp-accent)'}}/>}
                  </span>
                  <Icon name="printer" size={16} color="var(--mp-textMuted)"/>
                  <div style={{flex:1}}>
                    <div style={{fontSize:13, fontWeight:500}}>{p.name}</div>
                    <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)'}}>{p.location} · {p.status}</div>
                  </div>
                  {i===0 && <MPBadge tone="accent">default</MPBadge>}
                </label>
              ))}
            </div>
          )}

          {step===2 && (
            <div style={{padding:14, background:'var(--mp-surface)', border:'1px solid var(--mp-border)', borderRadius:8}}>
              <div style={{display:'flex', alignItems:'center', gap:10, marginBottom:10}}>
                <MPSwitch checked={true} onChange={()=>{}}/>
                <span style={{fontSize:13, fontWeight:500}}>Add to Windows 11 right-click menu</span>
              </div>
              <div style={{padding:10, background:'var(--mp-surface2)', borderRadius:6, fontSize:11.5, color:'var(--mp-textMuted)', lineHeight:1.5}}>
                Right-click any printable file → <span className="mp-mono">Print with MaeclyPrint</span>.
                Works for any number of files. Removing the integration later is one click.
              </div>
            </div>
          )}

          {step===3 && (
            <MPCard padding={14}>
              <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:12, fontSize:12}}>
                <div><div style={{color:'var(--mp-textMuted)', fontSize:11, marginBottom:3}}>Profile name</div><div style={{fontWeight:500}}>My default</div></div>
                <div><div style={{color:'var(--mp-textMuted)', fontSize:11, marginBottom:3}}>Printer</div><div className="mp-mono">Brother HL-L2350DW</div></div>
                <div><div style={{color:'var(--mp-textMuted)', fontSize:11, marginBottom:3}}>Color</div><div>Black & white</div></div>
                <div><div style={{color:'var(--mp-textMuted)', fontSize:11, marginBottom:3}}>Sides</div><div>Long-edge duplex</div></div>
                <div><div style={{color:'var(--mp-textMuted)', fontSize:11, marginBottom:3}}>Paper</div><div className="mp-mono">A4</div></div>
                <div><div style={{color:'var(--mp-textMuted)', fontSize:11, marginBottom:3}}>Copies</div><div className="mp-mono">1</div></div>
              </div>
            </MPCard>
          )}

          {step===4 && (
            <MPCard padding={16}>
              <div style={{display:'flex', alignItems:'center', gap:10, marginBottom:10}}>
                <div style={{width:28,height:28,borderRadius:'50%',background:'var(--mp-successSoft)',display:'flex',alignItems:'center',justifyContent:'center'}}>
                  <Icon name="check" size={14} color="var(--mp-success)"/>
                </div>
                <span style={{fontSize:13, fontWeight:500}}>You're set up.</span>
              </div>
              <div className="mp-mono" style={{fontSize:11, color:'var(--mp-textDim)', lineHeight:1.7}}>
                <div>✓ 3 printers detected</div>
                <div>✓ Right-click integration installed</div>
                <div>✓ Default profile created</div>
              </div>
            </MPCard>
          )}
        </div>
      </div>

      <div style={{padding:'14px 18px', borderTop:'1px solid var(--mp-border)', display:'flex', justifyContent:'space-between'}}>
        <MPButton variant="ghost" icon="chevl" disabled={step===0}>Back</MPButton>
        <MPButton variant="primary" iconRight={step===4?undefined:'chevr'}>{step===4?'Open MaeclyPrint':'Continue'}</MPButton>
      </div>
    </div>
  );
}

// ─────────── TOASTS ───────────
function ToastStack({ toasts }) {
  return (
    <div style={{position:'absolute', right:14, bottom:14, display:'flex', flexDirection:'column', gap:8, zIndex:50, alignItems:'flex-end'}}>
      {toasts.map(t => <Toast key={t.id} {...t}/>)}
    </div>
  );
}
function Toast({ tone='info', title, body, action, progress, kind }) {
  const tones = {
    success: { ico:'check', col:'var(--mp-success)', soft:'var(--mp-successSoft)' },
    error:   { ico:'alert', col:'var(--mp-error)', soft:'var(--mp-errorSoft)' },
    warn:    { ico:'alert', col:'var(--mp-warn)', soft:'var(--mp-warnSoft)' },
    info:    { ico:'info', col:'var(--mp-info)', soft:'var(--mp-infoSoft)' },
    progress:{ ico:'printer', col:'var(--mp-accent)', soft:'var(--mp-accentSoft)' },
  }[tone] || {};
  return (
    <div style={{
      width:320, background:'var(--mp-surface)', border:'1px solid var(--mp-border)',
      borderRadius:8, boxShadow:'0 8px 24px rgba(0,0,0,0.10)',
      overflow:'hidden',
    }}>
      <div style={{padding:'12px 14px', display:'flex', gap:10}}>
        <div style={{width:24, height:24, borderRadius:'50%', background:tones.soft, display:'flex', alignItems:'center', justifyContent:'center', flexShrink:0}}>
          <Icon name={tones.ico} size={12} color={tones.col}/>
        </div>
        <div style={{flex:1, minWidth:0}}>
          <div style={{fontSize:12.5, fontWeight:500, marginBottom:2}}>{title}</div>
          <div style={{fontSize:11.5, color:'var(--mp-textMuted)', lineHeight:1.5}}>{body}</div>
          {action && <div style={{marginTop:8, display:'flex', gap:6}}>
            {action.map((a,i)=><MPButton key={i} variant={i===0?'secondary':'ghost'} size="sm">{a}</MPButton>)}
          </div>}
        </div>
        <Icon name="x" size={14} color="var(--mp-textDim)"/>
      </div>
      {progress!=null && (
        <div style={{height:3, background:'var(--mp-surface3)'}}>
          <div style={{height:'100%', width:`${progress*100}%`, background:tones.col, transition:'width .3s'}}/>
        </div>
      )}
    </div>
  );
}

function ToastShowcase() {
  const samples = [
    { tone:'progress', title:'Printing 4 files', body:'Q4-financial-summary.docx (2 of 4)', progress:0.45 },
    { tone:'success', title:'Job complete', body:'4 files printed to Brother HL-L2350DW', action:['Show queue'] },
    { tone:'warn', title:'Printer offline', body:'HP OfficeJet 8025 didn\'t respond. Job paused.', action:['Retry','Choose another'] },
    { tone:'error', title:'Couldn\'t convert file', body:'budget-2026.xlsx — LibreOffice timed out at 60s.', action:['Retry','Open log'] },
    { tone:'info', title:'Update available', body:'MaeclyPrint 0.2.0 is ready to install.', action:['Install','Later'] },
  ];
  return (
    <div style={{
      width:'100%', height:'100%', padding:20,
      background:'var(--mp-bg)', display:'flex', flexDirection:'column', gap:10,
    }}>
      {samples.map((s,i) => <Toast key={i} {...s}/>)}
    </div>
  );
}

function ErrorState({ kind='no-printer' }) {
  const variants = {
    'no-printer': { ico:'printer-off', title:'No printers detected',
      body:'MaeclyPrint couldn\'t find any printers. Check that they\'re powered on and on the same network.',
      actions:[{label:'Refresh', primary:true},{label:'Add manually'}] },
    'conversion-failed': { ico:'alert', title:'Conversion failed',
      body:'Couldn\'t convert budget-2026.xlsx to PDF. LibreOffice exited with code 137 (out of memory).',
      actions:[{label:'Retry', primary:true},{label:'Open log'},{label:'Skip file'}] },
    'job-jammed': { ico:'alert', title:'Paper jam',
      body:'Brother HL-L2350DW reports a paper jam at tray 1. The job will resume automatically.',
      actions:[{label:'Open printer status', primary:true},{label:'Cancel job'}] },
  }[kind];
  return (
    <div style={{flex:1, display:'flex', alignItems:'center', justifyContent:'center', padding:40, background:'var(--mp-bg)'}}>
      <div style={{maxWidth:380, textAlign:'center'}}>
        <div style={{width:56, height:56, borderRadius:14, background:'var(--mp-surface)', border:'1px solid var(--mp-border)', display:'inline-flex', alignItems:'center', justifyContent:'center', marginBottom:14}}>
          <Icon name={variants.ico==='printer-off'?'printer':'alert'} size={24} color="var(--mp-textMuted)"/>
        </div>
        <h2 style={{margin:'0 0 6px', fontSize:16, fontWeight:600}}>{variants.title}</h2>
        <p style={{margin:'0 0 18px', fontSize:12.5, color:'var(--mp-textMuted)', lineHeight:1.55}}>{variants.body}</p>
        <div style={{display:'flex', gap:6, justifyContent:'center'}}>
          {variants.actions.map((a,i)=><MPButton key={i} variant={a.primary?'primary':'ghost'} size="sm">{a.label}</MPButton>)}
        </div>
      </div>
    </div>
  );
}

// ─────────── AUTO-UPDATE ───────────
function UpdateScreen({ phase='available' }) {
  if (phase==='checking') return (
    <div style={{flex:1, display:'flex', alignItems:'center', justifyContent:'center', padding:40, background:'var(--mp-bg)'}}>
      <div style={{textAlign:'center'}}>
        <div style={{width:48, height:48, border:'2.5px solid var(--mp-surface3)', borderTopColor:'var(--mp-accent)', borderRadius:'50%', margin:'0 auto 14px', animation:'mp-spin 0.9s linear infinite'}}/>
        <div style={{fontSize:14, fontWeight:500}}>Checking for updates…</div>
        <div className="mp-mono" style={{fontSize:11, color:'var(--mp-textDim)', marginTop:4}}>release.maecly.dev/winx64</div>
      </div>
    </div>
  );

  if (phase==='installing') return (
    <div style={{flex:1, display:'flex', alignItems:'center', justifyContent:'center', padding:40, background:'var(--mp-bg)'}}>
      <div style={{maxWidth:380, width:'100%'}}>
        <div style={{textAlign:'center', marginBottom:18}}>
          <MPAppIcon size={48}/>
          <div style={{fontSize:15, fontWeight:600, marginTop:10}}>Installing MaeclyPrint 0.2.0</div>
          <div className="mp-mono" style={{fontSize:11, color:'var(--mp-textDim)', marginTop:4}}>downloaded · verified · extracting</div>
        </div>
        <div style={{height:6, background:'var(--mp-surface3)', borderRadius:3, overflow:'hidden', marginBottom:8}}>
          <div style={{height:'100%', width:'68%', background:'var(--mp-accent)', transition:'width .5s'}}/>
        </div>
        <div style={{display:'flex', justifyContent:'space-between', fontSize:11, color:'var(--mp-textMuted)'}}>
          <span className="mp-mono">22.1 / 32.4 MiB</span>
          <span className="mp-mono">~12s remaining</span>
        </div>
      </div>
    </div>
  );

  // available
  return (
    <div style={{flex:1, display:'flex', flexDirection:'column', background:'var(--mp-bg)'}}>
      <MPTopBar title="Update available" subtitle="MaeclyPrint 0.2.0"/>
      <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:'18px 22px'}}>
        <MPCard padding={16} style={{marginBottom:18}}>
          <div style={{display:'flex', alignItems:'center', gap:12}}>
            <MPAppIcon size={40}/>
            <div style={{flex:1}}>
              <div style={{fontSize:13.5, fontWeight:500}}>0.1.0 → 0.2.0</div>
              <div className="mp-mono" style={{fontSize:11, color:'var(--mp-textDim)', marginTop:2}}>32.4 MiB · signed · stable channel</div>
            </div>
            <div style={{display:'flex', gap:6}}>
              <MPButton variant="ghost" size="sm">Later</MPButton>
              <MPButton variant="primary" icon="download" size="sm">Install & restart</MPButton>
            </div>
          </div>
        </MPCard>

        <MPSectionLabel>What's new</MPSectionLabel>
        <div style={{fontSize:12.5, lineHeight:1.7, color:'var(--mp-text)'}}>
          {[
            { t:'Print preview pane', d:'Inspect rendered output before sending to the printer.' },
            { t:'Batch templates', d:'Apply a different profile per file type in one job.' },
            { t:'macOS public beta', d:'First Apple Silicon build with Finder Quick Action.' },
            { t:'Faster Excel conversion', d:'New LibreOffice macro pipeline — 3.2× faster on large workbooks.' },
            { t:'Bug fixes', d:'12 issues closed. See full changelog on GitHub.' },
          ].map((it,i)=>(
            <div key={i} style={{display:'flex', gap:10, padding:'8px 0', borderBottom:i<4?'1px solid var(--mp-border)':'none'}}>
              <div style={{width:6, height:6, borderRadius:'50%', background:'var(--mp-accent)', marginTop:7, flexShrink:0}}/>
              <div>
                <div style={{fontWeight:500}}>{it.t}</div>
                <div style={{color:'var(--mp-textMuted)', fontSize:12, marginTop:2}}>{it.d}</div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

// ─────────── macOS FINDER ───────────
function FinderQuickAction() {
  const items = [
    { ico:'open', label:'Open' },
    { ico:'page', label:'Open With', sub:true },
    { ico:'page', label:'Move to Trash' },
    { sep:true },
    { ico:'page', label:'Get Info' },
    { ico:'rename', label:'Rename' },
    { ico:'page', label:'Compress 4 Items' },
    { sep:true },
    { ico:'mp', label:'Quick Actions', sub:true, mpInside:true },
    { sep:true },
    { ico:'copy', label:'Copy 4 Items' },
    { ico:'share', label:'Share', sub:true },
  ];
  return (
    <div style={{
      width:'100%', height:'100%', position:'relative',
      background:'linear-gradient(180deg,#3a3833 0%,#2c2a26 100%)',
      fontFamily:'-apple-system,"SF Pro Text",sans-serif', overflow:'hidden',
    }}>
      {/* faux finder titlebar */}
      <div style={{height:28, background:'rgba(60,60,60,0.85)', borderBottom:'1px solid rgba(0,0,0,0.4)', display:'flex', alignItems:'center', padding:'0 12px', gap:8}}>
        <span style={{width:11,height:11,borderRadius:'50%',background:'#ED6A5E'}}/>
        <span style={{width:11,height:11,borderRadius:'50%',background:'#F5BF4F'}}/>
        <span style={{width:11,height:11,borderRadius:'50%',background:'#62C554'}}/>
        <span style={{flex:1, textAlign:'center', fontSize:12, color:'#DCDCDC', fontWeight:500}}>Reports</span>
      </div>
      {/* selected files faux */}
      <div style={{padding:'30px 30px', display:'grid', gridTemplateColumns:'repeat(4,1fr)', gap:24}}>
        {[{ext:'docx',n:'Q4-summary'},{ext:'xlsx',n:'budget-2026'},{ext:'pdf',n:'team-roster'},{ext:'pptx',n:'roadmap'}].map((f,i)=>(
          <div key={i} style={{display:'flex', flexDirection:'column', alignItems:'center', gap:6}}>
            <div style={{width:54, height:54, background:'rgba(0,122,255,0.30)', border:'1.5px solid rgba(0,122,255,0.7)', borderRadius:6, display:'flex', alignItems:'center', justifyContent:'center'}}>
              <FileGlyph ext={f.ext} size={36}/>
            </div>
            <span style={{fontSize:11, color:'#FFF', textAlign:'center', background:'rgba(0,122,255,0.7)', padding:'1px 6px', borderRadius:3}}>{f.n}.{f.ext}</span>
          </div>
        ))}
      </div>
      {/* context menu */}
      <div style={{
        position:'absolute', left:140, top:120, width:230,
        background:'rgba(46,46,46,0.78)', backdropFilter:'blur(40px)',
        border:'0.5px solid rgba(255,255,255,0.10)', borderRadius:8,
        boxShadow:'0 12px 32px rgba(0,0,0,0.5)', padding:'4px', fontSize:13, color:'#FFF',
      }}>
        {items.map((it,i) => it.sep ? (
          <div key={i} style={{height:1, background:'rgba(255,255,255,0.10)', margin:'4px 8px'}}/>
        ) : (
          <div key={i} style={{
            display:'flex', alignItems:'center', gap:8, padding:'4px 10px', borderRadius:4,
            background: it.mpInside?'rgba(0,122,255,0.6)':'transparent',
          }}>
            {it.mpInside ? <MPGlyph size={12}/> : <Icon name={it.ico==='mp'?'page':it.ico} size={12} color="rgba(255,255,255,0.7)"/>}
            <span style={{flex:1, fontSize:12.5}}>{it.label}</span>
            {it.sub && <Icon name="chevr" size={10} color="rgba(255,255,255,0.7)"/>}
          </div>
        ))}
      </div>
      {/* sub menu showing MP entry */}
      <div style={{
        position:'absolute', left:370, top:265, width:230,
        background:'rgba(46,46,46,0.78)', backdropFilter:'blur(40px)',
        border:'0.5px solid rgba(255,255,255,0.10)', borderRadius:8,
        boxShadow:'0 12px 32px rgba(0,0,0,0.5)', padding:'4px', fontSize:13, color:'#FFF',
      }}>
        {[
          { ico:'mp', label:'Print with MaeclyPrint', highlight:true },
          { sep:true },
          { label:'Convert Image' },
          { label:'Create PDF' },
          { label:'Customize…', dim:true },
        ].map((it,i) => it.sep ? <div key={i} style={{height:1, background:'rgba(255,255,255,0.10)', margin:'4px 8px'}}/> : (
          <div key={i} style={{
            display:'flex', alignItems:'center', gap:8, padding:'4px 10px', borderRadius:4,
            background: it.highlight?'rgba(0,122,255,0.6)':'transparent',
            color: it.dim?'rgba(255,255,255,0.5)':'#FFF',
          }}>
            {it.ico==='mp' && <MPGlyph size={12}/>}
            <span style={{flex:1, fontSize:12.5}}>{it.label}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

Object.assign(window, {
  Win11Context, ContextMenuStage, ScreenOnboarding,
  Toast, ToastStack, ToastShowcase, ErrorState,
  UpdateScreen, FinderQuickAction,
});
