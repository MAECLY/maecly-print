// MaeclyPrint — Printers management screen
// Master-detail like Profiles. Capabilities from driver, default printer,
// per-printer tray/paper defaults, toner levels, test page, add/remove,
// missing-printer reconciliation, capability mismatch warnings.

function typeMeta(t) {
  return ({
    laser:   { label:'Laser',        ico:'printer' },
    mfp:     { label:'Multifunction',ico:'printer' },
    wide:    { label:'Wide format',  ico:'image' },
    label:   { label:'Label',        ico:'page' },
    virtual: { label:'Virtual',      ico:'file' },
  })[t] || { label:t, ico:'printer' };
}
function statusMeta(s) {
  return ({
    ready:   { tone:'success', label:'Ready',       dot:'done' },
    offline: { tone:'neutral', label:'Offline',     dot:'cancelled' },
    error:   { tone:'error',   label:'Error',       dot:'failed' },
    busy:    { tone:'accent',  label:'Printing',    dot:'printing' },
  })[s] || { tone:'neutral', label:s, dot:'queued' };
}
const TONER_COLORS = { cyan:'#0E9CC0', magenta:'#C81E5B', yellow:'#D9A300', black:'#2A2620' };

function LevelBar({ label, value, color }) {
  return (
    <div style={{display:'flex', alignItems:'center', gap:8}}>
      <span className="mp-mono" style={{fontSize:10, color:'var(--mp-textDim)', width:54, textTransform:'capitalize'}}>{label}</span>
      <div style={{flex:1, height:6, background:'var(--mp-surface3)', borderRadius:3, overflow:'hidden'}}>
        <div style={{height:'100%', width:`${value*100}%`, background:color, transition:'width .3s'}}/>
      </div>
      <span className="mp-mono" style={{fontSize:10, color: value<0.2?'var(--mp-error)':'var(--mp-textMuted)', width:30, textAlign:'right'}}>{Math.round(value*100)}%</span>
    </div>
  );
}

function CapPill({ ok, children }) {
  return (
    <div style={{
      display:'inline-flex', alignItems:'center', gap:5, padding:'4px 9px',
      borderRadius:5, fontSize:11.5,
      background: ok?'var(--mp-successSoft)':'var(--mp-surface2)',
      color: ok?'var(--mp-success)':'var(--mp-textDim)',
      border:`1px solid ${ok?'transparent':'var(--mp-border)'}`,
    }}>
      <Icon name={ok?'check':'x'} size={11}/>
      {children}
    </div>
  );
}

function ScreenPrinters({ selectedId='hp404', onSelect, mode='default', onMode, defaultId='hp404', onSetDefault }) {
  const printers = MP_PRINTERS;
  const sel = printers.find(p=>p.id===selectedId) || printers[0];
  const tm = typeMeta(sel.type);
  const sm = statusMeta(sel.status);
  const isDefault = sel.id === defaultId;

  const Block = ({ label, children, action }) => (
    <div style={{marginBottom:18}}>
      <div style={{display:'flex', justifyContent:'space-between', alignItems:'center', marginBottom:8}}>
        <MPSectionLabel>{label}</MPSectionLabel>
        {action}
      </div>
      {children}
    </div>
  );
  const Row = ({ label, children }) => (
    <div style={{display:'grid', gridTemplateColumns:'120px 1fr', gap:14, padding:'9px 0', borderBottom:'1px solid var(--mp-border)', alignItems:'center'}}>
      <span style={{fontSize:12, color:'var(--mp-textMuted)'}}>{label}</span>
      <span style={{fontSize:12.5}}>{children}</span>
    </div>
  );

  return (
    <>
      <MPTopBar title="Printers" subtitle={`${printers.length} configured · ${printers.filter(p=>p.status==='ready').length} ready`} right={
        <>
          <MPButton variant="ghost" icon="rotate" size="sm">Rescan</MPButton>
          <MPButton variant="primary" icon="plus" size="sm" onClick={()=>onMode?.('add-network')}>Add printer</MPButton>
        </>
      }/>
      <div style={{flex:1, display:'flex', minHeight:0, position:'relative'}}>
        {/* Master list */}
        <div className="mp-scroll" style={{width:264, flexShrink:0, overflow:'auto', borderRight:'1px solid var(--mp-border)', padding:'10px 8px', background:'var(--mp-surface2)'}}>
          {printers.map(p => {
            const a = p.id===sel.id; const s = statusMeta(p.status); const t = typeMeta(p.type);
            return (
              <button key={p.id} onClick={()=>onSelect?.(p.id)} className="mp-focus" style={{
                width:'100%', display:'flex', alignItems:'center', gap:10, padding:'9px 10px',
                border:'none', background:a?'var(--mp-surface)':'transparent', borderRadius:6,
                cursor:'pointer', textAlign:'left', marginBottom:2,
                boxShadow: a ? '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px var(--mp-border)' : 'none',
              }}>
                <div style={{width:30, height:30, borderRadius:6, background:'var(--mp-surface2)', border:'1px solid var(--mp-border)', display:'flex', alignItems:'center', justifyContent:'center', flexShrink:0}}>
                  <Icon name={t.ico} size={15} color="var(--mp-textMuted)"/>
                </div>
                <div style={{flex:1, minWidth:0}}>
                  <div style={{display:'flex', alignItems:'center', gap:6}}>
                    <span style={{fontSize:12.5, fontWeight:500, overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>{p.name}</span>
                    {p.id===defaultId && <Icon name="star" size={11} color="var(--mp-accent)"/>}
                  </div>
                  <div style={{display:'flex', alignItems:'center', gap:5, marginTop:2}}>
                    <StatusDot status={s.dot}/>
                    <span className="mp-mono" style={{fontSize:10, color:'var(--mp-textDim)'}}>{s.label} · {t.label}</span>
                  </div>
                </div>
              </button>
            );
          })}
          <button onClick={()=>onMode?.('add-network')} className="mp-focus" style={{
            width:'100%', display:'flex', alignItems:'center', gap:8, padding:'9px 10px',
            border:'1px dashed var(--mp-borderStrong)', background:'transparent', borderRadius:6,
            cursor:'pointer', marginTop:6, color:'var(--mp-textMuted)', fontSize:12.5,
          }}>
            <Icon name="plus" size={14}/> Add a printer
          </button>
        </div>

        {/* Detail */}
        <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:'18px 22px'}}>
          {/* Header */}
          <div style={{display:'flex', alignItems:'flex-start', justifyContent:'space-between', marginBottom:16, gap:12}}>
            <div style={{display:'flex', gap:12, minWidth:0}}>
              <div style={{width:44, height:44, borderRadius:9, background:'var(--mp-surface2)', border:'1px solid var(--mp-border)', display:'flex', alignItems:'center', justifyContent:'center', flexShrink:0}}>
                <Icon name={tm.ico} size={22} color="var(--mp-textMuted)"/>
              </div>
              <div style={{minWidth:0}}>
                <div style={{display:'flex', alignItems:'center', gap:8, flexWrap:'wrap'}}>
                  <h2 style={{margin:0, fontSize:17, fontWeight:600, letterSpacing:'-0.01em'}}>{sel.name}</h2>
                  <MPBadge tone={sm.tone} icon={sm.dot==='done'?'check':sm.dot==='failed'?'alert':undefined}>{sm.label}</MPBadge>
                  {isDefault && <MPBadge tone="accent" icon="star">default</MPBadge>}
                </div>
                <div className="mp-mono" style={{fontSize:11, color:'var(--mp-textDim)', marginTop:3}}>{tm.label} · {sel.conn} · queue {sel.queue}</div>
              </div>
            </div>
            <div style={{display:'flex', gap:6, flexShrink:0}}>
              <MPButton variant="ghost" icon="printer" size="sm">Test page</MPButton>
              <MPButton variant="ghost" icon="star" size="sm" disabled={isDefault} onClick={()=>onSetDefault?.(sel.id)}>Set default</MPButton>
              {sel.type!=='virtual' && <MPButton variant="danger" icon="trash" size="sm">Remove</MPButton>}
            </div>
          </div>

          {/* Missing-printer reconciliation banner */}
          {mode==='missing' && (
            <div style={{display:'flex', gap:10, padding:'12px 14px', background:'var(--mp-warnSoft)', border:'1px solid var(--mp-warn)', borderRadius:8, marginBottom:18}}>
              <Icon name="alert" size={16} color="var(--mp-warn)"/>
              <div style={{flex:1}}>
                <div style={{fontSize:12.5, fontWeight:500, color:'var(--mp-warn)'}}>Printer referenced by a profile is no longer available</div>
                <div style={{fontSize:12, color:'var(--mp-text)', marginTop:3, lineHeight:1.5}}>
                  Profile <strong>Press A · Duplex Long</strong> points to <span className="mp-mono">Sharp MX-3071</span>, which the system spooler no longer reports. Jobs using it are <strong>paused</strong>, not failed.
                </div>
                <div style={{display:'flex', gap:6, marginTop:10, alignItems:'center'}}>
                  <span style={{fontSize:11.5, color:'var(--mp-textMuted)'}}>Reassign to</span>
                  <MPSelect size="sm" value="brother" onChange={()=>{}} options={MP_PRINTERS.filter(p=>p.status==='ready').map(p=>({value:p.id,label:p.name}))}/>
                  <MPButton variant="primary" size="sm">Update profile</MPButton>
                  <MPButton variant="ghost" size="sm">Keep paused</MPButton>
                </div>
              </div>
            </div>
          )}

          {/* Org-managed default note (policy.json) */}
          {sel.id==='hp404' && mode!=='missing' && (
            <div style={{display:'flex', gap:8, padding:'9px 12px', background:'var(--mp-surface2)', border:'1px solid var(--mp-border)', borderRadius:6, marginBottom:18, alignItems:'center'}}>
              <Icon name="lock" size={13} color="var(--mp-textMuted)"/>
              <span style={{fontSize:11.5, color:'var(--mp-textMuted)'}}>Default printer is managed by your organization via <span className="mp-mono">policy.json</span>.</span>
            </div>
          )}

          <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:24}}>
            {/* Left column */}
            <div>
              <Block label="Connection">
                <Row label="Driver"><span className="mp-mono" style={{fontSize:11.5}}>{sel.driver}</span></Row>
                <Row label="Port / address"><span className="mp-mono" style={{fontSize:11.5}}>{sel.conn}</span></Row>
                <Row label="Location">{sel.location}</Row>
                <Row label="Shared">{sel.shared ? 'Yes — on LAN' : 'No'}</Row>
                <Row label="Max resolution"><span className="mp-mono" style={{fontSize:11.5}}>{sel.maxDpi} dpi</span></Row>
              </Block>

              <Block label="Defaults for this printer">
                <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:10}}>
                  <div>
                    <div style={{fontSize:11, color:'var(--mp-textMuted)', marginBottom:5}}>Default tray</div>
                    <MPSelect fullWidth value={sel.defTray} onChange={()=>{}} options={sel.trays.map(t=>({value:t.name,label:t.name}))}/>
                  </div>
                  <div>
                    <div style={{fontSize:11, color:'var(--mp-textMuted)', marginBottom:5}}>Default paper</div>
                    <MPSelect fullWidth value={sel.defPaper} onChange={()=>{}} options={sel.papers.map(p=>({value:p,label:p}))}/>
                  </div>
                </div>
              </Block>

              <Block label="Route file types here">
                <div style={{display:'flex', flexWrap:'wrap', gap:6}}>
                  {(sel.id==='sharp'?['.pdf','.jpg','.png']:sel.id==='zebra'?['.zpl']:sel.id==='hp404'?['.docx','.xlsx']:[]).map(x=>(
                    <span key={x} className="mp-mono" style={{fontSize:11, padding:'3px 8px', background:'var(--mp-accentSoft)', color:'var(--mp-accent)', borderRadius:5}}>{x} <Icon name="x" size={9}/></span>
                  ))}
                  <button className="mp-focus" style={{fontSize:11, padding:'3px 8px', background:'transparent', border:'1px dashed var(--mp-borderStrong)', borderRadius:5, color:'var(--mp-textMuted)', cursor:'pointer', display:'inline-flex', alignItems:'center', gap:4}}>
                    <Icon name="plus" size={10}/> add rule
                  </button>
                </div>
                <div style={{fontSize:11, color:'var(--mp-textDim)', marginTop:8, lineHeight:1.5}}>
                  Files with these extensions default to this printer when no profile overrides it.
                </div>
              </Block>
            </div>

            {/* Right column */}
            <div>
              <Block label="Capabilities" action={<span className="mp-mono" style={{fontSize:9.5, color:'var(--mp-textDim)'}}>queried from driver</span>}>
                <div style={{display:'flex', flexWrap:'wrap', gap:6, marginBottom:12}}>
                  <CapPill ok={sel.duplex}>Duplex {sel.duplex?'':'unavailable'}</CapPill>
                  <CapPill ok={sel.color}>{sel.color?'Color':'Monochrome only'}</CapPill>
                  <CapPill ok={true}>{sel.papers.length} paper sizes</CapPill>
                  <CapPill ok={sel.trays.length>1}>{sel.trays.length} {sel.trays.length===1?'tray':'trays'}</CapPill>
                </div>
                {!sel.duplex && (
                  <div style={{display:'flex', gap:7, padding:'8px 10px', background:'var(--mp-warnSoft)', borderRadius:6, marginBottom:8}}>
                    <Icon name="info" size={12} color="var(--mp-warn)"/>
                    <span style={{fontSize:11, color:'var(--mp-warn)', lineHeight:1.45}}>Profiles requesting duplex will pause with <span className="mp-mono">DuplexNotSupported</span> on this device.</span>
                  </div>
                )}
                <div style={{display:'flex', flexWrap:'wrap', gap:5}}>
                  {sel.papers.map(p=>(
                    <span key={p} className="mp-mono" style={{fontSize:10.5, padding:'3px 7px', background:'var(--mp-surface2)', border:'1px solid var(--mp-border)', borderRadius:4, color:'var(--mp-textMuted)'}}>{p}</span>
                  ))}
                </div>
              </Block>

              {Object.keys(sel.levels).length>0 && (
                <Block label="Supply levels">
                  <div style={{display:'flex', flexDirection:'column', gap:8, padding:'4px 0'}}>
                    {Object.entries(sel.levels).map(([k,v])=>(
                      <LevelBar key={k} label={k} value={v} color={TONER_COLORS[k]||'var(--mp-text)'}/>
                    ))}
                  </div>
                </Block>
              )}

              <Block label="Input trays">
                <MPCard padding={0}>
                  {sel.trays.map((t,i)=>(
                    <div key={t.name} style={{display:'flex', alignItems:'center', gap:10, padding:'9px 12px', borderBottom:i<sel.trays.length-1?'1px solid var(--mp-border)':'none'}}>
                      <Icon name="file" size={13} color="var(--mp-textDim)"/>
                      <span style={{fontSize:12, fontWeight:500, width:64}}>{t.name}</span>
                      <span className="mp-mono" style={{fontSize:11, color:'var(--mp-textMuted)', flex:1}}>{t.paper}</span>
                      <div style={{width:46, height:5, background:'var(--mp-surface3)', borderRadius:3, overflow:'hidden'}}>
                        <div style={{height:'100%', width:`${t.level*100}%`, background: t.level<0.2?'var(--mp-error)':'var(--mp-success)'}}/>
                      </div>
                    </div>
                  ))}
                </MPCard>
              </Block>
            </div>
          </div>
        </div>

        {/* Add-printer modal */}
        {(mode==='add-network' || mode==='add-ip' || mode==='add-system') && (
          <AddPrinterModal mode={mode} onMode={onMode}/>
        )}
      </div>
    </>
  );
}

function AddPrinterModal({ mode='add-network', onMode }) {
  const tab = mode==='add-ip' ? 'ip' : mode==='add-system' ? 'system' : 'network';
  return (
    <div style={{position:'absolute', inset:0, background:'rgba(20,18,14,0.42)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:40, backdropFilter:'blur(2px)'}}>
      <div className="mp-scroll" style={{width:480, maxHeight:'88%', overflow:'auto', background:'var(--mp-surface)', border:'1px solid var(--mp-border)', borderRadius:12, boxShadow:'0 24px 60px rgba(0,0,0,0.28)'}}>
        <div style={{display:'flex', alignItems:'center', justifyContent:'space-between', padding:'14px 16px', borderBottom:'1px solid var(--mp-border)'}}>
          <div style={{fontSize:14, fontWeight:600}}>Add a printer</div>
          <MPIconButton icon="x" size={26} label="Close" onClick={()=>onMode?.('default')}/>
        </div>

        <div style={{padding:'14px 16px'}}>
          <MPSegmented value={tab} onChange={(v)=>onMode?.(v==='ip'?'add-ip':v==='system'?'add-system':'add-network')} options={[
            {value:'network', label:'Scan network'},
            {value:'ip', label:'By IP address'},
            {value:'system', label:'From Windows'},
          ]}/>

          <div style={{height:14}}/>

          {tab==='network' && (
            <>
              <div style={{display:'flex', alignItems:'center', gap:8, marginBottom:10}}>
                <div style={{width:14, height:14, border:'2px solid var(--mp-surface3)', borderTopColor:'var(--mp-accent)', borderRadius:'50%', animation:'mp-spin 0.9s linear infinite'}}/>
                <span style={{fontSize:12, color:'var(--mp-textMuted)'}}>Scanning LAN via mDNS / WSD…</span>
              </div>
              <MPCard padding={0}>
                {MP_DISCOVERED.map((d,i)=>(
                  <div key={d.id} style={{display:'flex', alignItems:'center', gap:10, padding:'11px 12px', borderBottom:i<MP_DISCOVERED.length-1?'1px solid var(--mp-border)':'none'}}>
                    <div style={{width:30, height:30, borderRadius:6, background:'var(--mp-surface2)', display:'flex', alignItems:'center', justifyContent:'center'}}>
                      <Icon name={typeMeta(d.type).ico} size={15} color="var(--mp-textMuted)"/>
                    </div>
                    <div style={{flex:1, minWidth:0}}>
                      <div style={{fontSize:12.5, fontWeight:500}}>{d.name}</div>
                      <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)'}}>{d.conn} · {d.via}</div>
                    </div>
                    <MPButton variant="secondary" size="sm" icon="plus">Add</MPButton>
                  </div>
                ))}
              </MPCard>
            </>
          )}

          {tab==='ip' && (
            <div style={{display:'flex', flexDirection:'column', gap:12}}>
              <div>
                <div style={{fontSize:11.5, color:'var(--mp-textMuted)', marginBottom:5}}>Hostname or IP address</div>
                <MPInput fullWidth mono placeholder="10.0.0.40" value=""/>
              </div>
              <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:10}}>
                <div>
                  <div style={{fontSize:11.5, color:'var(--mp-textMuted)', marginBottom:5}}>Protocol</div>
                  <MPSelect fullWidth value="ipp" options={[{value:'ipp',label:'IPP / IPPS'},{value:'raw',label:'Raw (9100)'},{value:'lpd',label:'LPD/LPR'},{value:'wsd',label:'WSD'}]}/>
                </div>
                <div>
                  <div style={{fontSize:11.5, color:'var(--mp-textMuted)', marginBottom:5}}>Port</div>
                  <MPInput fullWidth mono value="631"/>
                </div>
              </div>
              <div>
                <div style={{fontSize:11.5, color:'var(--mp-textMuted)', marginBottom:5}}>Queue name (optional)</div>
                <MPInput fullWidth mono placeholder="e.g. PASSTHROUGH" value=""/>
              </div>
              <div style={{display:'flex', gap:7, padding:'9px 11px', background:'var(--mp-infoSoft)', borderRadius:6}}>
                <Icon name="info" size={13} color="var(--mp-info)"/>
                <span style={{fontSize:11, color:'var(--mp-info)', lineHeight:1.45}}>MaeclyPrint will query the driver for capabilities (duplex, color, trays) once connected.</span>
              </div>
            </div>
          )}

          {tab==='system' && (
            <>
              <div style={{fontSize:11.5, color:'var(--mp-textMuted)', marginBottom:10}}>Printers already known to the Windows spooler. Tick any to manage them in MaeclyPrint.</div>
              <MPCard padding={0}>
                {['Microsoft XPS Document Writer','Fax','OneNote (Desktop)','Adobe PDF'].map((n,i,arr)=>(
                  <label key={n} style={{display:'flex', alignItems:'center', gap:10, padding:'10px 12px', borderBottom:i<arr.length-1?'1px solid var(--mp-border)':'none', cursor:'pointer'}}>
                    <MPCheckbox checked={i===0} onChange={()=>{}}/>
                    <Icon name="printer" size={14} color="var(--mp-textMuted)"/>
                    <span style={{fontSize:12.5}}>{n}</span>
                  </label>
                ))}
              </MPCard>
            </>
          )}
        </div>

        <div style={{display:'flex', justifyContent:'flex-end', gap:8, padding:'12px 16px', borderTop:'1px solid var(--mp-border)'}}>
          <MPButton variant="ghost" onClick={()=>onMode?.('default')}>Cancel</MPButton>
          <MPButton variant="primary" icon="plus">Add printer</MPButton>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { ScreenPrinters, AddPrinterModal });
