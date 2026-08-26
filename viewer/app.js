// WorldSim viewer — vanilla WebGL2, no dependencies.
const $=s=>document.getElementById(s);
const canvas=$("canvas");
const gl=canvas.getContext("webgl2",{antialias:true,alpha:false});
if(!gl){ document.body.innerHTML="<pre style='padding:20px'>WebGL2 not available.</pre>"; throw new Error("no webgl2"); }

let apiBase=""; // same origin
let currentWorld=null, ws=null, wsRetry=0;
let state={tick:0,bodies:{n:0,px:null,py:null,pz:null,r:null,mat:null}, fluid:{n:0,px:null,py:null,pz:null, speed:null}, stats:null};

// --- camera ---
let cam={yaw:0.6,pitch:0.38,dist:9,target:[0,1,0], fov: 52*Math.PI/180};
function fitCamera(bounds){
  if(!bounds) return;
  const c=[(bounds.min[0]+bounds.max[0])/2,(bounds.min[1]+bounds.max[1])/2,(bounds.min[2]+bounds.max[2])/2];
  cam.target=c;
  const size=Math.max(bounds.max[0]-bounds.min[0], bounds.max[1]-bounds.min[1], bounds.max[2]-bounds.min[2]);
  cam.dist=size*1.45;
  cam.yaw=0.65; cam.pitch=0.42;
}

function resize(){
  const dpr=window.devicePixelRatio||1;
  const w=canvas.clientWidth*dpr, h=canvas.clientHeight*dpr;
  if(canvas.width!==w||canvas.height!==h){ canvas.width=w; canvas.height=h; gl.viewport(0,0,w,h);}
}
window.addEventListener("resize", resize); resize();

// --- GL programs ---
function compile(type,src){
  const s=gl.createShader(type); gl.shaderSource(s,src); gl.compileShader(s);
  if(!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
  return s;
}
function program(vs,fs){
  const p=gl.createProgram(); gl.attachShader(p,compile(gl.VERTEX_SHADER,vs)); gl.attachShader(p,compile(gl.FRAGMENT_SHADER,fs)); gl.linkProgram(p);
  if(!gl.getProgramParameter(p,gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  return p;
}
const vsPoints=`#version 300 es
layout(location=0) in vec3 aPos;
layout(location=1) in float aRad;
layout(location=2) in float aKind; // 0=bodies,1=fluid
layout(location=3) in float aScalar; // speed for fluid, mat for bodies
uniform mat4 uVP; uniform vec3 uEye; uniform float uPixRatio;
out vec3 vColor; out float vRadius;
void main(){
  vec4 w=uVP*vec4(aPos,1.0);
  gl_Position=w;
  // size by kind: bodies use radius, fluid fixed
  float worldSize = aKind<0.5 ? aRad*2.0 : 0.07;
  float dist=length(uEye-aPos);
  float pix = worldSize * 520.0 / max(dist,0.4); // tuned for 60° fov
  if(aKind>0.5) pix*= (0.7+ 0.6*clamp(aScalar/4.0,0.0,1.0));
  gl_PointSize=clamp(pix, 2.5, 26.0);
  vRadius=gl_PointSize;
  if(aKind<0.5){
    // palette for bodies: 0 steel,1 alu,2 copper,3 wood,4 rubber,5 glass,6 ice,7 ceramic
    vec3 pal[8]=vec3[8](vec3(0.58,0.60,0.63),vec3(0.78,0.79,0.80),vec3(0.85,0.53,0.35),vec3(0.62,0.44,0.26),vec3(0.22,0.22,0.24),vec3(0.75,0.88,0.90),vec3(0.80,0.92,0.98),vec3(0.90,0.87,0.80));
    int idx=int(clamp(aScalar,0.0,7.0));
    vColor=pal[idx];
  } else {
    // fluid: speed heatmap blue->cyan->white
    float t=clamp(aScalar/3.5,0.0,1.0);
    vec3 c0=vec3(0.17,0.46,0.84), c1=vec3(0.32,0.84,0.96), c2=vec3(0.96,0.98,1.0);
    vColor = t<0.5 ? mix(c0,c1,t*2.0) : mix(c1,c2,(t-0.5)*2.0);
    // tint by height slightly for depth cue
    float h=clamp((aPos.y+2.0)/4.0,0.0,1.0);
    vColor*= (0.82+0.22*h);
  }
}
`;
const fsPoints=`#version 300 es
precision mediump float;
in vec3 vColor; in float vRadius;
out vec4 frag;
void main(){
  vec2 c=gl_PointCoord*2.0-1.0;
  float d=dot(c,c);
  if(d>1.0) discard;
  float edge=smoothstep(1.0,0.82, 1.0-d);
  float spec=pow(max(1.0-length(c*1.25),0.0), 22.0)*0.55;
  vec3 col=vColor + vec3(spec);
  // soft shadow toward bottom for bodies: fake AO by darkening lower rim
  frag=vec4(col, edge*0.96);
}
`;
const progPoints=program(vsPoints,fsPoints);
const locPoints={
  vp: gl.getUniformLocation(progPoints,"uVP"),
  eye: gl.getUniformLocation(progPoints,"uEye"),
  pix: gl.getUniformLocation(progPoints,"uPixRatio"),
};
const vsLines=`#version 300 es
layout(location=0) in vec3 aPos;
uniform mat4 uVP;
void main(){ gl_Position=uVP*vec4(aPos,1.0); }`;
const fsLines=`#version 300 es
precision mediump float;
uniform vec3 uColor;
out vec4 frag;
void main(){ frag=vec4(uColor,1.0); }`;
const progLines=program(vsLines,fsLines);
const locLines={ vp: gl.getUniformLocation(progLines,"uVP"), color: gl.getUniformLocation(progLines,"uColor") };

// Buffers
let bufPoints=gl.createBuffer();
let lineVbo=gl.createBuffer();
let lineCountForBounds=0, lineCountForGrid=0;
let boundsCache=null;

function buildLineGeometry(bounds){
  if(!bounds) return;
  const min=bounds.min, max=bounds.max;
  // box 12 edges
  const c=[
    [min[0],min[1],min[2]],[max[0],min[1],min[2]],[max[0],min[1],max[2]],[min[0],min[1],max[2]],
    [min[0],max[1],min[2]],[max[0],max[1],min[2]],[max[0],max[1],max[2]],[min[0],max[1],max[2]],
  ];
  const edges=[[0,1],[1,2],[2,3],[3,0],[4,5],[5,6],[6,7],[7,4],[0,4],[1,5],[2,6],[3,7]];
  const verts=[];
  for(const [a,b] of edges){ verts.push(...c[a],...c[b]); }
  // grid on floor y=min[1]+0.01
  const y=min[1]+0.015;
  const step=0.5;
  const nx=Math.round((max[0]-min[0])/step), nz=Math.round((max[2]-min[2])/step);
  for(let i=0;i<=nx;i++){ const x=min[0]+i*step; verts.push(x,y,min[2], x,y,max[2]); }
  for(let i=0;i<=nz;i++){ const z=min[2]+i*step; verts.push(min[0],y,z, max[0],y,z); }
  lineCountForBounds=edges.length*2;
  lineCountForGrid=(nx+1 + nz+1)*2;
  gl.bindBuffer(gl.ARRAY_BUFFER, lineVbo);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(verts), gl.STATIC_DRAW);
  boundsCache=bounds;
}

function b64ToF32(b64){
  if(!b64) return null;
  try{
    const bin=atob(b64);
    const bytes=new Uint8Array(bin.length);
    for(let i=0;i<bin.length;i++) bytes[i]=bin.charCodeAt(i);
    return new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength/4);
  }catch(_){ return null; }
}
function b64ToU8(b64){
  if(!b64) return null;
  try{ const bin=atob(b64); const a=new Uint8Array(bin.length); for(let i=0;i<bin.length;i++) a[i]=bin.charCodeAt(i); return a;}catch(_){return null;}
}

function connectWS(worldId){
  if(ws){ try{ws.close();}catch(_){} ws=null; }
  if(!worldId) return;
  const proto=location.protocol==="https:"?"wss:":"ws:";
  const url=`${proto}//${location.host}/v1/worlds/${worldId}/stream`;
  ws=new WebSocket(url);
  ws.binaryType="arraybuffer";
  ws.onopen=()=>{ wsRetry=0; updateStatus("live"); };
  ws.onmessage=(ev)=>{
    try{
      const msg=JSON.parse(ev.data);
      if(msg.t==="frame"||msg.tick!==undefined){ ingestFrame(msg); }
    }catch(_){}
  };
  ws.onclose=()=>{ updateStatus("reconnecting…"); setTimeout(()=>connectWS(worldId), 700+Math.min(wsRetry++*420,4000)); };
  ws.onerror=()=>{ try{ws.close();}catch(_){} };
}

function ingestFrame(f){
  // tolerate two shapes: {tick,time,bodies:{n,p:...,r:...,mat:...}, fluid:{n,p:...,speed:...}, stats, bounds}
  // server sends base64 f32 p arrays as "bodies_px" etc plus optional legacy "bodies.p"
  // Normalise into state
  if(typeof f.tick==="number") state.tick=f.tick;
  if(f.stats) state.stats=f.stats;
  if(f.bounds) {
    if(!boundsCache || JSON.stringify(boundsCache)!==JSON.stringify(f.bounds)) {
      fitCamera(f.bounds); buildLineGeometry(f.bounds);
    }
  }
  // bodies
  if(f.bodies){
    const b=f.bodies;
    state.bodies.n=b.n||0;
    const px=b64ToF32(b.px||b.px_b64||b.p), py=b64ToF32(b.py||b.py_b64), pz=b64ToF32(b.pz||b.pz_b64);
    const r=b64ToF32(b.r||b.r_b64), mat=b64ToU8(b.mat||b.mat_b64);
    if(px) { state.bodies.px=px; state.bodies.py=py; state.bodies.pz=pz; state.bodies.r=r; state.bodies.mat=mat; }
  }
  // fluid
  if(f.fluid){
    const fl=f.fluid;
    state.fluid.n=fl.n||0;
    const px=b64ToF32(fl.px||fl.px_b64||fl.p), py=b64ToF32(fl.py||fl.py_b64), pz=b64ToF32(fl.pz||fl.pz_b64);
    const speed=b64ToF32(fl.speed||fl.speed_b64||fl.s);
    if(px){ state.fluid.px=px; state.fluid.py=py; state.fluid.pz=pz; state.fluid.speed=speed; }
  } else if(typeof f.fluid_n==="number"){
    state.fluid.n=f.fluid_n;
  }
  // also support flat arrays: f.bodies_px etc legacy path
  if(f.bodies_px){ const px=b64ToF32(f.bodies_px); if(px){ state.bodies.px=px; state.bodies.py=b64ToF32(f.bodies_py); state.bodies.pz=b64ToF32(f.bodies_pz); state.bodies.n=px.length; }}
  updateHUD();
}

function updateHUD(){
  const s=state.stats;
  const stats = s ? `tick ${state.tick}  t=${(s.time||0).toFixed(2)}s  tps ${(s.tps||0).toFixed(1)}  dt ${(s.tick_ms||0).toFixed(1)} ms\nbodies ${state.bodies.n}  fluid ${state.fluid.n}  quality ${s.quality||"?"} ${s.auto?"(auto)":"(locked)"}` : `tick ${state.tick}\nbodies ${state.bodies.n}  fluid ${state.fluid.n}`;
  $("hud-stats").textContent=stats;
  if(currentWorld) $("hud-world").textContent=`world ${currentWorld.id.slice(0,8)} · ${currentWorld.name||"unnamed"}`;
}

function updateStatus(t){ $("controls-status").textContent=t; }

function matVP(){
  const aspect=canvas.width/Math.max(1,canvas.height);
  const f=Math.tan(cam.fov/2);
  // view
  const eye=(()=>{ const cy=Math.cos(cam.yaw), sy=Math.sin(cam.yaw), cp=Math.cos(cam.pitch), sp=Math.sin(cam.pitch); const x=cy*cp*cam.dist, y=sp*cam.dist, z=sy*cp*cam.dist; return [cam.target[0]+x, cam.target[1]+y, cam.target[2]+z];})();
  const center=cam.target;
  const up=[0,1,0];
  // lookAt
  function normalize(v){ const l=Math.hypot(...v); return l>0?[v[0]/l,v[1]/l,v[2]/l]:[0,1,0]; }
  function cross(a,b){ return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]; }
  function sub(a,b){ return [a[0]-b[0],a[1]-b[1],a[2]-b[2]]; }
  const fwd=normalize(sub(center,eye));
  const right=normalize(cross(fwd,up));
  const up2=cross(right,fwd);
  const view=[
    right[0],up2[0],-fwd[0],0,
    right[1],up2[1],-fwd[1],0,
    right[2],up2[2],-fwd[2],0,
    - (right[0]*eye[0]+right[1]*eye[1]+right[2]*eye[2]),
    - (up2[0]*eye[0]+up2[1]*eye[1]+up2[2]*eye[2]),
      (fwd[0]*eye[0]+fwd[1]*eye[1]+fwd[2]*eye[2]),
    1
  ];
  // proj (column-major)
  const near=0.08, far=120;
  const yScale=1/f, xScale=yScale/aspect;
  const proj=[xScale,0,0,0, 0,yScale,0,0, 0,0,-(far+near)/(far-near),-1, 0,0,-2*far*near/(far-near),0];
  // vp = proj * view
  const vp=new Float32Array(16);
  for(let i=0;i<4;i++) for(let j=0;j<4;j++){ let s=0; for(let k=0;k<4;k++) s+=proj[i+k*4]*view[k+j*4]; vp[i+j*4]=s; }
  return {vp,eye};
}

function render(){
  requestAnimationFrame(render);
  resize();
  gl.clearColor(0.045,0.07,0.11,1); gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);
  gl.enable(gl.DEPTH_TEST); gl.enable(gl.BLEND); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA); gl.enable(gl.PROGRAM_POINT_SIZE);
  const {vp,eye}=matVP();
  // lines: bounds + grid
  if(lineCountForBounds>0){
    gl.useProgram(progLines); gl.uniformMatrix4fv(locLines.vp,false,vp);
    gl.bindBuffer(gl.ARRAY_BUFFER, lineVbo);
    gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0,3,gl.FLOAT,false,0,0);
    // bounds box
    gl.uniform3f(locLines.color, 0.22,0.32,0.48);
    gl.drawArrays(gl.LINES, 0, lineCountForBounds);
    // grid
    gl.uniform3f(locLines.color, 0.13,0.20,0.32);
    gl.drawArrays(gl.LINES, lineCountForBounds, lineCountForGrid);
  }
  // points
  gl.useProgram(progPoints);
  gl.uniformMatrix4fv(locPoints.vp,false,vp);
  gl.uniform3f(locPoints.eye, eye[0],eye[1],eye[2]);
  gl.uniform1f(locPoints.pix, window.devicePixelRatio||1);
  // assemble interleaved arrays for bodies + fluid in two draw calls? Simpler two calls.
  function drawSet(px,py,pz, rOrNull, matOrSpeed, kind){
    const n=px?px.length:0; if(!n) return;
    const pos=new Float32Array(n*3);
    for(let i=0;i<n;i++){ pos[i*3]=px[i]; pos[i*3+1]=py[i]; pos[i*3+2]=pz[i]; }
    const rad=rOrNull || new Float32Array(n);
    if(kind===1 && !rOrNull){ rad.fill(0.06); }
    const scalar=matOrSpeed || new Float32Array(n);
    // vao-less: non-interleaved separate attribs would need strides; we interleave manually per draw?
    // Simpler: three separate buffers.
    const bPos=gl.createBuffer(), bRad=gl.createBuffer(), bSca=gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, bPos); gl.bufferData(gl.ARRAY_BUFFER, pos, gl.DYNAMIC_DRAW); gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0,3,gl.FLOAT,false,0,0);
    gl.bindBuffer(gl.ARRAY_BUFFER, bRad); gl.bufferData(gl.ARRAY_BUFFER, rad, gl.DYNAMIC_DRAW); gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1,1,gl.FLOAT,false,0,0);
    gl.bindBuffer(gl.ARRAY_BUFFER, bSca); gl.bufferData(gl.ARRAY_BUFFER, scalar, gl.DYNAMIC_DRAW); gl.enableVertexAttribArray(2); gl.vertexAttribPointer(2,1,gl.FLOAT,false,0,0);
    // constant kind attribute via vertexAttrib1f + disable array trick: bind and disable? Simpler pass via uniform + branch: we already branch on kind via aKind attribute value.
    // Fill aKind buffer
    const aKind=new Float32Array(n); aKind.fill(kind);
    const bKind=gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER,bKind); gl.bufferData(gl.ARRAY_BUFFER,aKind,gl.DYNAMIC_DRAW); gl.enableVertexAttribArray(3); gl.vertexAttribPointer(3,1,gl.FLOAT,false,0,0);
    // Actually shader reads location 2 as aKind and 3 as scalar — need remap: we used location mapping: 0 pos,1 rad,2 kind,3 scalar
    // So rebind: rad->1, kind->2, scalar->3. Above we mis-wired; fix by reassigning:
    // Rebind correctly:
    // already bound 0=pos, 1=rad, 2=scalar (was kind), 3=kind (was scalar) -> swap
    // Instead recreate correctly quickly:
    // We'll disable and re-setup: pos0, rad1, kind2, scalar3
    gl.bindBuffer(gl.ARRAY_BUFFER,bRad); gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1,1,gl.FLOAT,false,0,0);
    gl.bindBuffer(gl.ARRAY_BUFFER,bKind); gl.enableVertexAttribArray(2); gl.vertexAttribPointer(2,1,gl.FLOAT,false,0,0);
    gl.bindBuffer(gl.ARRAY_BUFFER,bSca); gl.enableVertexAttribArray(3); gl.vertexAttribPointer(3,1,gl.FLOAT,false,0,0);
    gl.drawArrays(gl.POINTS,0,n);
    gl.deleteBuffer(bPos); gl.deleteBuffer(bRad); gl.deleteBuffer(bSca); gl.deleteBuffer(bKind);
  }
  drawSet(state.bodies.px,state.bodies.py,state.bodies.pz, state.bodies.r, state.bodies.mat?new Float32Array(state.bodies.mat):null, 0);
  drawSet(state.fluid.px,state.fluid.py,state.fluid.pz, null, state.fluid.speed, 1);
}
render();

// --- interactions: orbit ---
let dragging=false, lastX=0,lastY=0;
canvas.addEventListener("mousedown",e=>{ if(e.button!==0) return; dragging=true; lastX=e.clientX; lastY=e.clientY; });
window.addEventListener("mouseup",()=>dragging=false);
canvas.addEventListener("mousemove",e=>{
  if(!dragging) return;
  const dx=e.clientX-lastX, dy=e.clientY-lastY; lastX=e.clientX; lastY=e.clientY;
  cam.yaw+=dx*0.0045; cam.pitch=Math.max(-1.35,Math.min(1.35,cam.pitch+dy*0.0045));
});
canvas.addEventListener("wheel",e=>{ cam.dist=Math.max(0.6,Math.min(70,cam.dist* (e.deltaY>0?1.08:0.92))); e.preventDefault(); },{passive:false});

// click to spawn: ray vs y=ground plane (bounds min.y)
canvas.addEventListener("click",e=>{
  if(dragging) return;
  if(!currentWorld) return;
  const kind=$("sel-spawn").value;
  if(kind==="heat"||kind==="fluid") return; // these handled via button at center
  // unproject
  const rect=canvas.getBoundingClientRect();
  const x=( (e.clientX-rect.left)/rect.width)*2-1;
  const y= -(( (e.clientY-rect.top)/rect.height)*2-1);
  const {vp,eye}=matVP();
  // inverse vp
  function invert(m){
    const inv=new Float32Array(16);
    const b=[
      m[0]*m[5]-m[1]*m[4], m[0]*m[6]-m[2]*m[4], m[0]*m[7]-m[3]*m[4],
      m[1]*m[6]-m[2]*m[5], m[1]*m[7]-m[3]*m[5], m[2]*m[7]-m[3]*m[6],
      m[8]*m[13]-m[9]*m[12], m[8]*m[14]-m[10]*m[12], m[8]*m[15]-m[11]*m[12],
      m[9]*m[14]-m[10]*m[13], m[9]*m[15]-m[11]*m[13], m[10]*m[15]-m[11]*m[14]
    ];
    let det=b[0]*b[11]-b[1]*b[10]+b[2]*b[9]+b[3]*b[8]-b[4]*b[7]+b[5]*b[6];
    if(Math.abs(det)<1e-12) return null;
    det=1/det;
    inv[0]=(m[5]*b[11]-m[6]*b[10]+m[7]*b[9])*det;
    inv[1]=(-m[1]*b[11]+m[2]*b[10]-m[3]*b[9])*det;
    inv[2]=(m[13]*b[5]-m[14]*b[4]+m[15]*b[3])*det;
    inv[3]=(-m[9]*b[5]+m[10]*b[4]-m[11]*b[3])*det;
    inv[4]=(-m[4]*b[11]+m[6]*b[8]-m[7]*b[7])*det;
    inv[5]=(m[0]*b[11]-m[2]*b[8]+m[3]*b[7])*det;
    inv[6]=(-m[12]*b[5]+m[14]*b[2]-m[15]*b[1])*det;
    inv[7]=(m[8]*b[5]-m[10]*b[2]+m[11]*b[1])*det;
    inv[8]=(m[4]*b[10]-m[5]*b[8]+m[7]*b[6])*det;
    inv[9]=(-m[0]*b[10]+m[1]*b[8]-m[3]*b[6])*det;
    inv[10]=(m[12]*b[4]-m[13]*b[2]+m[15]*b[0])*det;
    inv[11]=(-m[8]*b[4]+m[9]*b[2]-m[11]*b[0])*det;
    inv[12]=(-m[4]*b[9]+m[5]*b[7]-m[6]*b[6])*det;
    inv[13]=(m[0]*b[9]-m[1]*b[7]+m[2]*b[6])*det;
    inv[14]=(-m[12]*b[3]+m[13]*b[1]-m[14]*b[0])*det;
    inv[15]=(m[8]*b[3]-m[9]*b[1]+m[10]*b[0])*det;
    return inv;
  }
  const inv= invert(vp);
  if(!inv) return;
  function unproj(sx,sy,z){
    const o=[sx,sy,z,1];
    const r=[ inv[0]*o[0]+inv[4]*o[1]+inv[8]*o[2]+inv[12]*o[3], inv[1]*o[0]+inv[5]*o[1]+inv[9]*o[2]+inv[13]*o[3], inv[2]*o[0]+inv[6]*o[1]+inv[10]*o[2]+inv[14]*o[3], inv[3]*o[0]+inv[7]*o[1]+inv[11]*o[2]+inv[15]*o[3]];
    return r[3]!==0?[r[0]/r[3],r[1]/r[3],r[2]/r[3]]:null;
  }
  const n=unproj(x,y,-1), f=unproj(x,y,1);
  if(!n||!f) return;
  const dir=[f[0]-n[0],f[1]-n[1],f[2]-n[2]];
  // intersect y = boundsCache.min.y + 0.6 (a bit above ground)
  const bounds=boundsCache||{min:[-2,0,-2],max:[2,4,2]};
  const y0=bounds.min[1]+0.65;
  if(Math.abs(dir[1])<1e-6) return;
  const t=(y0 - n[1])/dir[1];
  if(t<0) return;
  const hit=[n[0]+dir[0]*t, y0, n[2]+dir[2]*t];
  // clamp inside bounds
  hit[0]=Math.max(bounds.min[0]+0.3,Math.min(bounds.max[0]-0.3,hit[0]));
  hit[2]=Math.max(bounds.min[2]+0.3,Math.min(bounds.max[2]-0.3,hit[2]));
  doSpawn(kind, hit);
});

function doSpawn(kind, pos){
  if(!currentWorld) return;
  const body={type:"spawn_sphere"};
  const r= kind==="wood"?0.22: kind==="rubber"?0.16:0.14;
  if(kind==="steel"||kind==="wood"||kind==="rubber"){
    const p=pos||[ (Math.random()-0.5)*1.2, 2.8, (Math.random()-0.5)*1.2 ];
    fetch(`${apiBase}/v1/worlds/${currentWorld.id}/events`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({events:[{type:"spawn_sphere",material:kind,pos:p,vel:[0,0,0],radius:r}]})}).catch(()=>{});
  } else if(kind==="fluid"){
    const c=pos||[0,1.6,0];
    fetch(`${apiBase}/v1/worlds/${currentWorld.id}/events`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({events:[{type:"spawn_fluid",center:c,radius:0.55}]})}).catch(()=>{});
  } else if(kind==="heat"){
    const c=pos||[ -0.9,0.35,0.2];
    fetch(`${apiBase}/v1/worlds/${currentWorld.id}/events`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({events:[{type:"heat_source",pos:c,radius:0.38,power_watts:8000,duration_ticks:0}]})}).catch(()=>{});
  }
}
$("btn-spawn").addEventListener("click",()=> doSpawn($("sel-spawn").value, null));
$("btn-reset").addEventListener("click",()=>{ if(boundsCache) fitCamera(boundsCache); });
$("btn-pause").addEventListener("click",()=>{
  if(!currentWorld) return;
  const want = $("btn-pause").textContent==="Pause";
  fetch(`${apiBase}/v1/worlds/${currentWorld.id}/control`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({action: want?"pause":"resume"})}).then(()=>{
    $("btn-pause").textContent= want?"Resume":"Pause";
  });
});
$("sel-rate").addEventListener("change",e=>{
  if(!currentWorld) return;
  fetch(`${apiBase}/v1/worlds/${currentWorld.id}/control`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({action:"set_rate",rate: parseFloat(e.target.value)})});
});
$("sel-quality").addEventListener("change",e=>{
  if(!currentWorld) return;
  fetch(`${apiBase}/v1/worlds/${currentWorld.id}/precision`,{method:"PUT",headers:{"Content-Type":"application/json"},body:JSON.stringify({quality:e.target.value})});
});

// world list
async function refreshWorlds(selectId){
  const r=await fetch(`${apiBase}/v1/worlds`).then(x=>x.json()).catch(()=>({worlds:[]}));
  const worlds=r.worlds||[];
  const sel=$("sel-world"); sel.innerHTML="";
  for(const w of worlds){
    const o=document.createElement("option"); o.value=w.id; o.textContent=`${(w.name||"world").slice(0,16)} · ${w.id.slice(0,6)} · tick ${w.tick}`;
    sel.appendChild(o);
  }
  if(worlds.length){
    const pick=selectId||worlds[0].id;
    sel.value=pick;
    selectWorld(pick);
  } else {
    currentWorld=null; if(ws) try{ws.close()}catch(_){}
    $("hud-stats").textContent="no worlds — create a demo";
  }
}
async function selectWorld(id){
  const r=await fetch(`${apiBase}/v1/worlds/${id}`).then(x=>x.json()).catch(()=>null);
  if(!r) return;
  currentWorld={id:r.id,name:r.name,bounds:r.config?r.config.bounds:null,config:r.config};
  if(r.config&&r.config.bounds) buildLineGeometry({min:r.config.bounds.min,max:r.config.bounds.max});
  connectWS(id);
  $("sel-quality").value=(r.config&&r.config.quality)||"medium";
}
$("sel-world").addEventListener("change",e=> selectWorld(e.target.value));
$("btn-create").addEventListener("click",async()=>{
  const r=await fetch(`${apiBase}/v1/worlds`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({name:"demo",config:{quality:"medium"}})}).then(x=>x.json()).catch(()=>null);
  if(!r) return;
  // seed demo events immediately
  const id=r.id;
  await fetch(`${apiBase}/v1/worlds/${id}/events`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({events:[
    {type:"spawn_fluid",center:[0,1.0,0],radius:0.52},
    {type:"spawn_sphere",material:"wood",pos:[0.35,2.9,0.2],vel:[0,0,0],radius:0.20},
    {type:"spawn_sphere",material:"steel",pos:[-0.45,2.9,-0.15],vel:[0,0,0],radius:0.14},
    {type:"spawn_sphere",material:"rubber",pos:[0,2.7,0.45],vel:[0,0,0],radius:0.15},
    {type:"heat_source",pos:[-0.95,0.35,0.1],radius:0.32,power_watts:7000,duration_ticks:0}
  ]})}).catch(()=>{});
  await refreshWorlds(id);
});
refreshWorlds();
setInterval(()=>{ if(!ws || ws.readyState!==1) return; }, 8000);
