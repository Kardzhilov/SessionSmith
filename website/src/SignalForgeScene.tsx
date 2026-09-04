import { useEffect, useRef } from 'react'
import * as THREE from 'three'

type SignalForgeSceneProps = {
  screenshotUrl: string
}

const waveformColors = [0xffc857, 0x79d2b8, 0xef6a50, 0xa8ead7, 0xffe29a]
const transcriptLines = [
  ['00:14:08', 'MARA', 'The bell is ringing under the water.'],
  ['00:14:13', 'ORIN', 'Then we know where the passage ends.'],
  ['00:14:21', 'GM', 'The fog folds around the lantern light.'],
  ['00:14:34', 'TAMSIN', 'I mark the archway on our map.'],
  ['00:15:02', 'MARA', 'Wait. Someone else is breathing.'],
]
type ArtifactKind = 'map' | 'character' | 'quest' | 'recap'

const campaignArtifacts: Array<{
  kind: ArtifactKind
  eyebrow: string
  title: string
  accent: string
}> = [
  { kind: 'map', eyebrow: 'ENCOUNTER MAP', title: 'ASHFALL KEEP', accent: '#3f8d76' },
  { kind: 'character', eyebrow: 'CHARACTER SHEET', title: 'MARA VALE', accent: '#d69d2f' },
  { kind: 'quest', eyebrow: 'OPEN THREAD', title: 'THE DROWNED GATE', accent: '#c95842' },
  { kind: 'recap', eyebrow: 'SESSION RECAP', title: 'THE BELL IN THE FOG', accent: '#3f8d76' },
]

function makeCardTexture(eyebrow: string, title: string, body: string, accent: string) {
  const canvas = document.createElement('canvas')
  canvas.width = 768
  canvas.height = 384
  const context = canvas.getContext('2d')
  if (!context) return null

  context.fillStyle = '#eef2ea'
  context.fillRect(0, 0, canvas.width, canvas.height)
  context.fillStyle = accent
  context.fillRect(0, 0, 18, canvas.height)
  context.fillStyle = '#1b2823'
  context.font = '700 30px sans-serif'
  context.fillText(eyebrow, 54, 68)
  context.fillStyle = '#567067'
  context.font = '600 22px sans-serif'
  context.fillText(title, 54, 118)
  context.fillStyle = '#1b2823'
  context.font = '500 31px sans-serif'
  context.fillText(body, 54, 190, 650)
  context.fillStyle = '#9aaba4'
  context.fillRect(54, 248, 590, 10)
  context.fillRect(54, 280, 505, 10)
  context.fillRect(54, 312, 385, 10)

  const texture = new THREE.CanvasTexture(canvas)
  texture.colorSpace = THREE.SRGBColorSpace
  texture.minFilter = THREE.LinearFilter
  return texture
}

function makeArtifactTexture(kind: ArtifactKind, eyebrow: string, title: string, accent: string) {
  const canvas = document.createElement('canvas')
  canvas.width = 768
  canvas.height = 512
  const context = canvas.getContext('2d')
  if (!context) return null

  context.fillStyle = '#eee9d6'
  context.fillRect(0, 0, canvas.width, canvas.height)
  context.fillStyle = accent
  context.fillRect(0, 0, canvas.width, 16)
  context.fillStyle = '#645f4e'
  context.font = '700 22px sans-serif'
  context.fillText(eyebrow, 42, 60)
  context.fillStyle = '#201f1a'
  context.font = '800 38px serif'
  context.fillText(title, 42, 108)
  context.strokeStyle = '#aaa28a'
  context.lineWidth = 2
  context.beginPath()
  context.moveTo(42, 130)
  context.lineTo(726, 130)
  context.stroke()

  if (kind === 'map') {
    context.strokeStyle = '#665f4f'
    context.lineWidth = 6
    context.strokeRect(86, 176, 170, 112)
    context.strokeRect(256, 205, 190, 144)
    context.strokeRect(446, 164, 210, 118)
    context.beginPath()
    context.moveTo(170, 288)
    context.lineTo(170, 398)
    context.lineTo(350, 398)
    context.lineTo(350, 349)
    context.moveTo(446, 245)
    context.lineTo(525, 245)
    context.lineTo(525, 385)
    context.stroke()
    context.fillStyle = '#c95842'
    context.beginPath()
    context.arc(348, 275, 14, 0, Math.PI * 2)
    context.fill()
    context.fillStyle = '#3f8d76'
    context.beginPath()
    context.arc(568, 222, 14, 0, Math.PI * 2)
    context.fill()
    context.fillStyle = '#645f4e'
    context.font = '600 18px sans-serif'
    context.fillText('BELL TOWER', 102, 250)
    context.fillText('DROWNED HALL', 278, 320)
    context.fillText('SEA GATE', 503, 258)
  }

  if (kind === 'character') {
    const stats = [['STR', '10'], ['DEX', '17'], ['WIS', '15'], ['AC', '16']]
    stats.forEach(([label, value], index) => {
      const x = 42 + index * 112
      context.strokeStyle = '#8b846f'
      context.lineWidth = 3
      context.strokeRect(x, 166, 86, 92)
      context.fillStyle = '#645f4e'
      context.font = '700 18px sans-serif'
      context.fillText(label, x + 24, 193)
      context.fillStyle = '#201f1a'
      context.font = '800 34px serif'
      context.fillText(value, x + 25, 238)
    })
    context.fillStyle = '#645f4e'
    context.font = '700 20px sans-serif'
    context.fillText('RANGER 7  /  WAYFARER', 42, 305)
    context.fillText('HIT POINTS', 42, 357)
    context.strokeStyle = '#8b846f'
    context.strokeRect(180, 332, 220, 32)
    context.fillStyle = accent
    context.fillRect(184, 336, 168, 24)
    context.fillStyle = '#645f4e'
    context.fillText('THE FOGBOUND COMPASS', 42, 426)
  }

  if (kind === 'quest') {
    const tasks = ['Find the flooded stair', 'Name the voice in the bell', 'Return before the spring tide']
    tasks.forEach((task, index) => {
      const y = 182 + index * 82
      context.strokeStyle = index === 0 ? accent : '#8b846f'
      context.lineWidth = 4
      context.strokeRect(48, y, 28, 28)
      if (index === 0) {
        context.beginPath()
        context.moveTo(54, y + 14)
        context.lineTo(64, y + 23)
        context.lineTo(80, y - 4)
        context.stroke()
      }
      context.fillStyle = '#302e26'
      context.font = '600 25px sans-serif'
      context.fillText(task, 102, y + 24)
    })
    context.fillStyle = '#645f4e'
    context.font = '700 18px sans-serif'
    context.fillText('REWARD: THE WARDEN\'S TRUE NAME', 48, 452)
  }

  if (kind === 'recap') {
    const beats = [
      'The party entered Ashfall beneath a silent moon.',
      'Mara heard a bell sounding below the flooded crypt.',
      'A promise was made to the last keeper of the gate.',
    ]
    beats.forEach((beat, index) => {
      const y = 185 + index * 86
      context.fillStyle = accent
      context.beginPath()
      context.arc(58, y - 7, 7, 0, Math.PI * 2)
      context.fill()
      context.fillStyle = '#302e26'
      context.font = '500 24px serif'
      context.fillText(beat, 84, y, 620)
      context.strokeStyle = '#c2baa2'
      context.lineWidth = 2
      context.beginPath()
      context.moveTo(84, y + 24)
      context.lineTo(690, y + 24)
      context.stroke()
    })
  }

  const texture = new THREE.CanvasTexture(canvas)
  texture.colorSpace = THREE.SRGBColorSpace
  texture.minFilter = THREE.LinearFilter
  return texture
}

function makeCampaignTokenTexture(label: string, accent: string) {
  const canvas = document.createElement('canvas')
  canvas.width = 256
  canvas.height = 256
  const context = canvas.getContext('2d')
  if (!context) return null

  context.fillStyle = '#202b27'
  context.beginPath()
  context.arc(128, 128, 120, 0, Math.PI * 2)
  context.fill()
  context.strokeStyle = accent
  context.lineWidth = 14
  context.stroke()
  context.fillStyle = '#eee9d6'
  context.font = `${label.length > 5 ? 700 : 800} ${label.length > 5 ? 34 : 43}px sans-serif`
  context.textAlign = 'center'
  context.textBaseline = 'middle'
  context.fillText(label, 128, 130, 190)

  const texture = new THREE.CanvasTexture(canvas)
  texture.colorSpace = THREE.SRGBColorSpace
  texture.minFilter = THREE.LinearFilter
  return texture
}

function makeDieNumberTexture(number: string, color: string) {
  const canvas = document.createElement('canvas')
  canvas.width = 256
  canvas.height = 128
  const context = canvas.getContext('2d')
  if (!context) return null

  context.fillStyle = 'rgba(7, 17, 15, 0.84)'
  context.fillRect(42, 14, 172, 100)
  context.strokeStyle = color
  context.lineWidth = 7
  context.strokeRect(42, 14, 172, 100)
  context.fillStyle = color
  context.font = '800 68px serif'
  context.textAlign = 'center'
  context.textBaseline = 'middle'
  context.fillText(number, 128, 67)

  const texture = new THREE.CanvasTexture(canvas)
  texture.colorSpace = THREE.SRGBColorSpace
  texture.minFilter = THREE.LinearFilter
  return texture
}

export default function SignalForgeScene({ screenshotUrl }: SignalForgeSceneProps) {
  const hostRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host) return

    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const scene = new THREE.Scene()
    scene.background = new THREE.Color(0x07110f)
    scene.fog = new THREE.FogExp2(0x07110f, 0.048)

    const camera = new THREE.PerspectiveCamera(42, 1, 0.1, 100)
    camera.position.set(0, 0, 11)

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, powerPreference: 'high-performance' })
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.75))
    renderer.outputColorSpace = THREE.SRGBColorSpace
    renderer.toneMapping = THREE.ACESFilmicToneMapping
    renderer.toneMappingExposure = 1.08
    host.appendChild(renderer.domElement)

    const geometries: THREE.BufferGeometry[] = []
    const materials: THREE.Material[] = []
    const textures: THREE.Texture[] = []
    const keepGeometry = <T extends THREE.BufferGeometry>(geometry: T) => {
      geometries.push(geometry)
      return geometry
    }
    const keepMaterial = <T extends THREE.Material>(material: T) => {
      materials.push(material)
      return material
    }

    const world = new THREE.Group()
    scene.add(world)
    scene.add(new THREE.HemisphereLight(0xd8fff5, 0x15221e, 2.2))
    const keyLight = new THREE.DirectionalLight(0xffd991, 3.4)
    keyLight.position.set(-4, 6, 8)
    scene.add(keyLight)
    const coralLight = new THREE.PointLight(0xef6a50, 16, 11, 2)
    coralLight.position.set(4, -3, 3)
    scene.add(coralLight)

    const fieldUniforms = {
      uTime: { value: 0 },
      uScroll: { value: 0 },
      uPointer: { value: new THREE.Vector2() },
    }
    const signalField = new THREE.Mesh(
      keepGeometry(new THREE.PlaneGeometry(26, 16)),
      keepMaterial(new THREE.ShaderMaterial({
        depthWrite: false,
        uniforms: fieldUniforms,
        vertexShader: `
          varying vec2 vUv;
          void main() {
            vUv = uv;
            gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
          }
        `,
        fragmentShader: `
          varying vec2 vUv;
          uniform float uTime;
          uniform float uScroll;
          uniform vec2 uPointer;
          void main() {
            vec2 uv = vUv - 0.5;
            uv.x *= 1.625;
            vec2 origin = vec2(0.18 + uPointer.x * 0.035, -0.02 - uPointer.y * 0.025);
            float distanceFromSignal = length(uv - origin);
            float torchlight = smoothstep(0.88, 0.04, distanceFromSignal);
            float vertical = smoothstep(0.482, 0.5, abs(fract(uv.x * 9.0) - 0.5));
            float diagonalA = smoothstep(0.482, 0.5, abs(fract((uv.x * 0.5 + uv.y * 0.866) * 9.0) - 0.5));
            float diagonalB = smoothstep(0.482, 0.5, abs(fract((uv.x * 0.5 - uv.y * 0.866) * 9.0) - 0.5));
            float hexMap = max(vertical, max(diagonalA, diagonalB));
            float route = smoothstep(0.025, 0.0, abs(uv.y - sin(uv.x * 3.0 + uTime * 0.16) * 0.08));
            vec3 color = vec3(0.018, 0.055, 0.047);
            color += vec3(0.05, 0.23, 0.18) * torchlight * 0.22;
            color += vec3(0.10, 0.31, 0.24) * hexMap * 0.12;
            color += mix(vec3(0.95, 0.34, 0.20), vec3(1.0, 0.74, 0.24), uScroll) * route * 0.2;
            gl_FragColor = vec4(color, 1.0);
          }
        `,
      })),
    )
    signalField.position.z = -7
    scene.add(signalField)

    const screenTexture = new THREE.TextureLoader().load(screenshotUrl)
    screenTexture.colorSpace = THREE.SRGBColorSpace
    screenTexture.anisotropy = renderer.capabilities.getMaxAnisotropy()
    textures.push(screenTexture)

    const screenFrame = new THREE.Mesh(
      keepGeometry(new THREE.BoxGeometry(6.5, 4.22, 0.16, 2, 2, 2)),
      keepMaterial(new THREE.MeshStandardMaterial({ color: 0x17201e, roughness: 0.28, metalness: 0.65 })),
    )
    const screen = new THREE.Mesh(
      keepGeometry(new THREE.PlaneGeometry(6.28, 4.04)),
      keepMaterial(new THREE.MeshBasicMaterial({ map: screenTexture, toneMapped: false })),
    )
    screen.position.z = 0.091
    const screenGlow = new THREE.Mesh(
      keepGeometry(new THREE.PlaneGeometry(7.25, 4.95)),
      keepMaterial(new THREE.MeshBasicMaterial({ color: 0x45c4a4, transparent: true, opacity: 0.11, depthWrite: false })),
    )
    screenGlow.position.z = -0.13
    const screenEdges = new THREE.LineSegments(
      keepGeometry(new THREE.EdgesGeometry(screenFrame.geometry)),
      keepMaterial(new THREE.LineBasicMaterial({ color: 0xa8ead7, transparent: true, opacity: 0.32 })),
    )
    const screenGroup = new THREE.Group()
    screenGroup.add(screenGlow, screenFrame, screen, screenEdges)
    screenGroup.position.set(2.7, -0.12, 0)
    screenGroup.rotation.set(-0.04, -0.2, 0.015)
    world.add(screenGroup)

    const captureGroup = new THREE.Group()
    captureGroup.position.set(-2.55, 0.15, -0.2)
    const microphone = new THREE.Group()
    const microphoneHead = new THREE.Mesh(
      keepGeometry(new THREE.CapsuleGeometry(0.38, 0.76, 8, 18)),
      keepMaterial(new THREE.MeshStandardMaterial({ color: 0xe8f2ed, roughness: 0.23, metalness: 0.72 })),
    )
    microphoneHead.rotation.z = 0.12
    const microphoneBand = new THREE.Mesh(
      keepGeometry(new THREE.TorusGeometry(0.55, 0.035, 8, 48, Math.PI * 1.55)),
      keepMaterial(new THREE.MeshBasicMaterial({ color: 0xffc857 })),
    )
    microphoneBand.rotation.z = -0.7
    const microphoneStem = new THREE.Mesh(
      keepGeometry(new THREE.CylinderGeometry(0.035, 0.05, 1.42, 12)),
      keepMaterial(new THREE.MeshStandardMaterial({ color: 0x79d2b8, roughness: 0.35, metalness: 0.65 })),
    )
    microphoneStem.position.set(0.09, -1, 0)
    microphoneStem.rotation.z = -0.12
    const microphoneBase = new THREE.Mesh(
      keepGeometry(new THREE.CylinderGeometry(0.57, 0.68, 0.08, 32)),
      keepMaterial(new THREE.MeshStandardMaterial({ color: 0x17201e, roughness: 0.4, metalness: 0.6 })),
    )
    microphoneBase.position.set(0.18, -1.72, 0)
    microphone.add(microphoneHead, microphoneBand, microphoneStem, microphoneBase)
    captureGroup.add(microphone)

    const miniatureColors = [0xef6a50, 0x79d2b8, 0xffc857, 0xe8f2ed, 0x6f8fc9, 0xd981b5]
    const miniatures = miniatureColors.map((color, index) => {
      const miniature = new THREE.Group()
      const base = new THREE.Mesh(
        keepGeometry(new THREE.CylinderGeometry(0.2, 0.23, 0.08, 24)),
        keepMaterial(new THREE.MeshStandardMaterial({ color: 0x202b27, roughness: 0.72 })),
      )
      base.rotation.x = Math.PI / 2
      const body = new THREE.Mesh(
        keepGeometry(index % 2 === 0 ? new THREE.ConeGeometry(0.13, 0.38, 10) : new THREE.CylinderGeometry(0.09, 0.13, 0.38, 10)),
        keepMaterial(new THREE.MeshStandardMaterial({ color, roughness: 0.58, metalness: 0.08 })),
      )
      body.position.y = 0.22
      const head = new THREE.Mesh(
        keepGeometry(new THREE.SphereGeometry(0.105, 12, 8)),
        keepMaterial(new THREE.MeshStandardMaterial({ color: 0xe8c9a2, roughness: 0.8 })),
      )
      head.position.y = 0.49
      miniature.add(base, body, head)
      captureGroup.add(miniature)
      return miniature
    })
    world.add(captureGroup)

    const waveforms = waveformColors.map((color, lineIndex) => {
      const geometry = keepGeometry(new THREE.BufferGeometry())
      geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(320 * 3), 3))
      const material = keepMaterial(new THREE.LineBasicMaterial({
        color,
        transparent: true,
        opacity: 0.76 - lineIndex * 0.09,
        blending: THREE.AdditiveBlending,
      }))
      const line = new THREE.Line(geometry, material)
      line.position.set(-1.8, 0, -0.3 - lineIndex * 0.19)
      world.add(line)
      return line
    })

    const transcriptGroup = new THREE.Group()
    const transcriptCards = transcriptLines.map(([timestamp, speaker, body], index) => {
      const texture = makeCardTexture(timestamp, speaker, body, index % 2 === 0 ? '#ef6a50' : '#45a98f')
      if (texture) textures.push(texture)
      const card = new THREE.Mesh(
        keepGeometry(new THREE.BoxGeometry(2.45, 1.22, 0.045)),
        keepMaterial(new THREE.MeshStandardMaterial({
          color: texture ? 0xffffff : 0xeef2ea,
          map: texture,
          roughness: 0.72,
          metalness: 0.02,
          transparent: true,
          opacity: 0,
        })),
      )
      transcriptGroup.add(card)
      return card
    })
    world.add(transcriptGroup)

    const documentGroup = new THREE.Group()
    const papers = campaignArtifacts.map(({ kind, eyebrow, title, accent }) => {
      const texture = makeArtifactTexture(kind, eyebrow, title, accent)
      if (texture) textures.push(texture)
      const paper = new THREE.Mesh(
        keepGeometry(new THREE.PlaneGeometry(2.25, 1.5, 14, 10)),
        keepMaterial(new THREE.MeshStandardMaterial({
          color: texture ? 0xffffff : 0xe6eee8,
          map: texture,
          roughness: 0.84,
          side: THREE.DoubleSide,
          transparent: true,
          opacity: 0,
        })),
      )
      documentGroup.add(paper)
      return paper
    })
    world.add(documentGroup)

    const memoryGroup = new THREE.Group()
    const memoryPositions = [
      [-3.8, 2.1, 0], [-2.6, 1.1, 0.3], [-1.1, 2.4, -0.2], [0.3, 1.25, 0.4],
      [1.8, 2.2, -0.4], [3.45, 1.15, 0.2], [-3.2, -0.5, -0.2], [-1.7, -1.45, 0.35],
      [0, -0.65, -0.25], [1.5, -1.55, 0.3], [3.25, -0.45, -0.2], [0.1, 0.25, 0.65],
    ].map(([x, y, z]) => new THREE.Vector3(x, y, z))
    const memoryLinks = [[0, 1], [1, 2], [1, 6], [2, 3], [3, 4], [3, 8], [3, 11], [4, 5], [5, 10], [6, 7], [7, 8], [8, 9], [8, 11], [9, 10], [10, 11]]
    const memoryLinePoints = memoryLinks.flatMap(([from, to]) => [memoryPositions[from], memoryPositions[to]])
    const memoryMaterial = keepMaterial(new THREE.LineBasicMaterial({ color: 0xef6a50, transparent: true, opacity: 0 }))
    const memoryLines = new THREE.LineSegments(keepGeometry(new THREE.BufferGeometry().setFromPoints(memoryLinePoints)), memoryMaterial)
    memoryGroup.add(memoryLines)
    const tokenLabels = ['KEEP', 'MARA', 'BELL', 'ORIN', 'WARDEN', 'GATE', 'CLUE', 'OATH', 'TIDE', 'MAP', 'KEY', 'SESSION']
    const memoryNodes = memoryPositions.map((position, index) => {
      const accent = index === 11 ? '#ffc857' : index % 4 === 0 ? '#ef6a50' : '#79d2b8'
      const texture = makeCampaignTokenTexture(tokenLabels[index], accent)
      if (texture) textures.push(texture)
      const node = new THREE.Mesh(
        keepGeometry(new THREE.CircleGeometry(index === 11 ? 0.3 : 0.22, 32)),
        keepMaterial(new THREE.MeshStandardMaterial({
          color: texture ? 0xffffff : 0x202b27,
          map: texture,
          roughness: 0.68,
          side: THREE.DoubleSide,
          transparent: true,
          opacity: 0,
        })),
      )
      node.position.copy(position)
      memoryGroup.add(node)
      return node
    })
    memoryGroup.position.set(0.7, -0.2, -0.5)
    world.add(memoryGroup)

    const dice = [
      { geometry: new THREE.IcosahedronGeometry(0.74, 0), color: 0xc99726, edge: 0xffdf83, number: '20', labelColor: '#ffdf83' },
      { geometry: new THREE.DodecahedronGeometry(0.48, 0), color: 0xa94435, edge: 0xff9c82, number: '12', labelColor: '#ff9c82' },
    ].map(({ geometry, color, edge, number, labelColor }) => {
      keepGeometry(geometry)
      const die = new THREE.Group()
      const body = new THREE.Mesh(
        geometry,
        keepMaterial(new THREE.MeshStandardMaterial({ color, roughness: 0.34, metalness: 0.28, flatShading: true })),
      )
      const edges = new THREE.LineSegments(
        keepGeometry(new THREE.EdgesGeometry(geometry, 8)),
        keepMaterial(new THREE.LineBasicMaterial({ color: edge, transparent: true, opacity: 0.72 })),
      )
      const labelTexture = makeDieNumberTexture(number, labelColor)
      if (labelTexture) textures.push(labelTexture)
      const label = new THREE.Sprite(
        keepMaterial(new THREE.SpriteMaterial({ map: labelTexture, transparent: true, depthTest: false })),
      )
      label.position.z = number === '20' ? 0.82 : 0.58
      label.scale.set(number === '20' ? 0.72 : 0.56, number === '20' ? 0.36 : 0.28, 1)
      die.add(body, edges, label)
      return die
    })
    dice[0].position.set(5.15, 3.05, -0.4)
    dice[1].position.set(-5.1, -3.15, -0.8)
    world.add(...dice)

    const scatteredDiceGeometry = keepGeometry(new THREE.TetrahedronGeometry(0.065, 0))
    const scatteredDiceMaterial = keepMaterial(new THREE.MeshBasicMaterial({
      color: 0xffc857,
      wireframe: true,
      transparent: true,
      opacity: 0.34,
    }))
    const scatteredDice = new THREE.InstancedMesh(scatteredDiceGeometry, scatteredDiceMaterial, 38)
    const dieTransform = new THREE.Object3D()
    for (let index = 0; index < 38; index += 1) {
      dieTransform.position.set(
        ((index * 47.13) % 15) - 7.5,
        ((index * 31.71) % 10) - 5,
        ((index * 19.37) % 8) - 6,
      )
      dieTransform.rotation.set(index * 0.37, index * 0.61, index * 0.23)
      dieTransform.scale.setScalar(0.6 + (index % 5) * 0.16)
      dieTransform.updateMatrix()
      scatteredDice.setMatrixAt(index, dieTransform.matrix)
    }
    scatteredDice.instanceMatrix.needsUpdate = true
    world.add(scatteredDice)

    let scrollProgress = 0
    let pointerX = 0
    let pointerY = 0
    let pointerEnergy = 0
    let pulseBoost = 0
    let animationFrame = 0
    let isMobile = false
    let isCompact = false

    const updateScroll = () => {
      const journey = document.querySelector<HTMLElement>('[data-scene-journey]')
      const end = journey ? journey.offsetTop + journey.offsetHeight : window.innerHeight * 3
      scrollProgress = THREE.MathUtils.clamp(window.scrollY / Math.max(end - window.innerHeight, 1), 0, 1)
    }

    const updatePointer = (event: PointerEvent) => {
      const nextX = (event.clientX / window.innerWidth - 0.5) * 2
      const nextY = (event.clientY / window.innerHeight - 0.5) * 2
      pointerEnergy = Math.min(1, pointerEnergy + Math.hypot(nextX - pointerX, nextY - pointerY) * 1.6)
      pointerX = nextX
      pointerY = nextY
    }

    const triggerPulse = () => {
      pulseBoost = 1
    }

    const resize = () => {
      const width = host.clientWidth
      const height = host.clientHeight
      isMobile = width < 760
      isCompact = width < 980
      renderer.setSize(width, height, false)
      camera.aspect = width / Math.max(height, 1)
      camera.fov = isMobile ? 49 : 42
      camera.updateProjectionMatrix()
    }

    const render = (time: number) => {
      const seconds = time * 0.001
      const motion = reducedMotion ? 0 : seconds
      const pointerWeight = reducedMotion ? 0 : 1
      const capturePhase = 1 - THREE.MathUtils.smoothstep(scrollProgress, 0.13, 0.39)
      const transcriptIn = THREE.MathUtils.smoothstep(scrollProgress, 0.17, 0.42)
      const transcriptOut = 1 - THREE.MathUtils.smoothstep(scrollProgress, 0.57, 0.76)
      const transcriptPhase = transcriptIn * transcriptOut
      const documentPhase = THREE.MathUtils.smoothstep(scrollProgress, 0.44, 0.75)
      const memoryPhase = THREE.MathUtils.smoothstep(scrollProgress, 0.68, 0.97)
      const baseScreenX = isMobile ? 1.05 : isCompact ? 1.9 : 2.7
      const baseScreenY = isMobile ? -1.45 : -0.12

      pointerEnergy *= 0.93
      pulseBoost *= 0.94
      fieldUniforms.uTime.value = motion
      fieldUniforms.uScroll.value = scrollProgress
      fieldUniforms.uPointer.value.set(pointerX * pointerWeight, pointerY * pointerWeight)

      waveforms.forEach((line, lineIndex) => {
        const position = line.geometry.attributes.position as THREE.BufferAttribute
        for (let pointIndex = 0; pointIndex < position.count; pointIndex += 1) {
          const ratio = pointIndex / (position.count - 1)
          const x = ratio * 13 - 6.5
          const envelope = Math.pow(Math.sin(ratio * Math.PI), 0.7)
          const frequency = 13 + lineIndex * 2.4 + scrollProgress * 15
          const amplitude = (0.22 + scrollProgress * 0.36 + pointerEnergy * 0.22) * envelope
          const carrier = Math.sin(ratio * frequency + motion * (1.35 + lineIndex * 0.18))
          const detail = Math.sin(ratio * 52 - motion * 2.1 + lineIndex) * 0.12
          position.setXYZ(
            pointIndex,
            x,
            (carrier + detail) * amplitude + (lineIndex - 2) * 0.16,
            Math.cos(ratio * 9 + motion * 0.5 + lineIndex) * 0.2,
          )
        }
        position.needsUpdate = true
        line.rotation.z = -0.08 + scrollProgress * 0.23
        line.position.y = 0.75 - scrollProgress * 2.2
        line.position.x = -1.8 + memoryPhase * 1.5
        ;(line.material as THREE.LineBasicMaterial).opacity = (0.72 - lineIndex * 0.08) * (1 - memoryPhase * 0.62)
      })

      const screenScale = (isMobile ? 0.52 : isCompact ? 0.78 : 1) * (1 - documentPhase * 0.28 - memoryPhase * 0.24)
      screenGroup.scale.setScalar(screenScale)
      screenGroup.position.x = baseScreenX - transcriptIn * (isMobile ? 0.35 : 1.05) - memoryPhase * 1.8
      screenGroup.position.y = baseScreenY + scrollProgress * (isMobile ? 0.55 : 0.42)
      screenGroup.position.z = -scrollProgress * 1.5
      screenGroup.rotation.y = -0.2 + pointerX * 0.04 * pointerWeight + scrollProgress * 0.34
      screenGroup.rotation.x = -0.04 - pointerY * 0.025 * pointerWeight - scrollProgress * 0.08
      screenGlow.scale.setScalar(1 + Math.sin(motion * 1.3) * 0.025 + pointerEnergy * 0.05)
      ;(screenGlow.material as THREE.MeshBasicMaterial).opacity = 0.09 + capturePhase * 0.08 + pointerEnergy * 0.05

      captureGroup.visible = capturePhase > 0.005
      captureGroup.scale.setScalar((isMobile ? 0.58 : isCompact ? 0.82 : 1) * (0.8 + capturePhase * 0.2))
      captureGroup.position.x = isMobile ? -1.85 : -2.55
      captureGroup.position.y = (isMobile ? 1.45 : 0.15) - (1 - capturePhase) * 1.1
      microphone.rotation.y = motion * 0.08 + pointerX * 0.12 * pointerWeight
      microphone.rotation.x = pointerY * -0.08 * pointerWeight
      miniatures.forEach((miniature, index) => {
        const angle = (index / miniatures.length) * Math.PI * 2 + motion * (index % 2 === 0 ? 0.035 : -0.025)
        const radius = 1.18 + (index % 2) * 0.26
        const hop = pulseBoost * Math.sin(Math.PI * Math.min(1, pulseBoost + index * 0.04)) * 0.22
        miniature.position.set(Math.cos(angle) * radius, Math.sin(angle) * radius + hop, -0.34 + (index % 3) * 0.08)
        miniature.rotation.z = -angle + Math.PI / 2
        miniature.scale.setScalar(0.8 + pointerEnergy * 0.18)
      })

      transcriptCards.forEach((card, index) => {
        const localPhase = THREE.MathUtils.smoothstep(transcriptIn, index * 0.11, 0.58 + index * 0.08)
        const angle = -0.62 + index * 0.31
        const radius = isMobile ? 1.9 : isCompact ? 2.55 : 3.25
        card.scale.setScalar(isMobile ? 0.62 : isCompact ? 0.78 : 1)
        card.position.set(
          (isMobile ? -0.7 : -1.4) + Math.cos(angle) * radius * localPhase,
          -0.25 + Math.sin(angle) * radius * localPhase + index * 0.2,
          isMobile ? -2.6 + localPhase * 0.7 : -1.8 + localPhase * (1.45 + index * 0.11),
        )
        card.rotation.set(-0.07 + index * 0.025, -0.22 + localPhase * 0.2, -0.16 + index * 0.075)
        const material = card.material as THREE.MeshStandardMaterial
        material.opacity = transcriptPhase * Math.min(1, localPhase * 2.5)
        card.visible = material.opacity > 0.008
      })

      papers.forEach((paper, index) => {
        const stagger = THREE.MathUtils.smoothstep(documentPhase, index * 0.12, 0.62 + index * 0.08)
        const side = index % 2 === 0 ? -1 : 1
        paper.scale.setScalar(isMobile ? 0.58 : isCompact ? 0.76 : 1)
        paper.position.set(
          (isMobile ? 0.15 : 1.2) + side * stagger * (0.65 + index * 0.42),
          -2.8 + stagger * (2.1 + (index % 2) * 1.15),
          isMobile ? -2.8 + stagger * 0.75 : -1.4 + stagger * (1.1 + index * 0.08),
        )
        paper.rotation.set(-0.12 + stagger * 0.08, side * (0.34 - stagger * 0.24), side * (-0.06 - index * 0.035))
        const material = paper.material as THREE.MeshStandardMaterial
        material.opacity = documentPhase * (1 - memoryPhase * 0.72) * Math.min(1, stagger * 2.5) * (isMobile ? 0.58 : 1)
        paper.visible = material.opacity > 0.008
      })

      memoryGroup.scale.setScalar((isMobile ? 0.58 : isCompact ? 0.8 : 1) * (0.72 + memoryPhase * 0.28))
      memoryGroup.position.x = isMobile ? 0 : 0.7
      memoryGroup.position.y = isMobile ? 0.55 : -0.2
      memoryGroup.rotation.y = pointerX * 0.08 * pointerWeight + motion * 0.018
      memoryGroup.rotation.x = pointerY * -0.05 * pointerWeight
      memoryMaterial.opacity = memoryPhase * 0.58
      memoryNodes.forEach((node, index) => {
        const nodePhase = THREE.MathUtils.smoothstep(memoryPhase, index * 0.035, 0.45 + index * 0.025)
        const material = node.material as THREE.MeshStandardMaterial
        material.opacity = nodePhase * 0.9
        const throb = 1 + Math.sin(motion * 1.8 + index * 0.9) * 0.12 + pointerEnergy * 0.18
        node.scale.setScalar(nodePhase * throb)
      })

      dice[0].position.x = isMobile ? 3.15 : 5.15
      dice[0].position.y = isMobile ? 2.45 : 3.05
      dice[1].position.x = isMobile ? -3.1 : -5.1
      dice.forEach((die, index) => {
        die.rotation.x = motion * (0.2 + index * 0.08) + scrollProgress * Math.PI * (1.2 + index * 0.45)
        die.rotation.y = motion * (0.31 - index * 0.06) + scrollProgress * Math.PI * (1.55 - index * 0.2)
        die.scale.setScalar(1 + pointerEnergy * 0.16 + Math.sin(motion + index) * 0.035)
      })

      scatteredDice.rotation.y = motion * 0.015 + scrollProgress * 0.16
      scatteredDice.rotation.z = scrollProgress * -0.05
      scatteredDiceMaterial.opacity = 0.22 + pointerEnergy * 0.18 + memoryPhase * 0.08
      world.rotation.y += (pointerX * 0.022 * pointerWeight - world.rotation.y) * 0.035
      world.rotation.x += (pointerY * -0.014 * pointerWeight - world.rotation.x) * 0.035

      renderer.render(scene, camera)
      animationFrame = window.requestAnimationFrame(render)
    }

    updateScroll()
    resize()
    window.addEventListener('resize', resize)
    window.addEventListener('scroll', updateScroll, { passive: true })
    window.addEventListener('pointermove', updatePointer, { passive: true })
    window.addEventListener('pointerdown', triggerPulse, { passive: true })
    animationFrame = window.requestAnimationFrame(render)

    return () => {
      window.cancelAnimationFrame(animationFrame)
      window.removeEventListener('resize', resize)
      window.removeEventListener('scroll', updateScroll)
      window.removeEventListener('pointermove', updatePointer)
      window.removeEventListener('pointerdown', triggerPulse)
      geometries.forEach((geometry) => geometry.dispose())
      materials.forEach((material) => material.dispose())
      textures.forEach((texture) => texture.dispose())
      renderer.dispose()
      renderer.domElement.remove()
    }
  }, [screenshotUrl])

  return <div className="scene-canvas" ref={hostRef} aria-hidden="true" />
}
