import { useEffect, useRef } from 'react'
import * as THREE from 'three'

type ScrollSceneProps = {
  screenshotUrl: string
}

const waveformColors = [0xffc857, 0x5bc8af, 0xf06c4f]

export default function ScrollScene({ screenshotUrl }: ScrollSceneProps) {
  const hostRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host) return

    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const scene = new THREE.Scene()
    scene.background = new THREE.Color(0x07110f)
    scene.fog = new THREE.FogExp2(0x07110f, 0.055)

    const camera = new THREE.PerspectiveCamera(42, 1, 0.1, 100)
    camera.position.set(0, 0, 11)

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false })
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.75))
    renderer.outputColorSpace = THREE.SRGBColorSpace
    renderer.setAnimationLoop(null)
    host.appendChild(renderer.domElement)

    const world = new THREE.Group()
    scene.add(world)

    scene.add(new THREE.HemisphereLight(0xd8fff5, 0x15221e, 2.1))
    const keyLight = new THREE.DirectionalLight(0xffd991, 3.2)
    keyLight.position.set(-4, 6, 8)
    scene.add(keyLight)

    const screenTexture = new THREE.TextureLoader().load(screenshotUrl)
    screenTexture.colorSpace = THREE.SRGBColorSpace
    screenTexture.anisotropy = renderer.capabilities.getMaxAnisotropy()

    const screenFrame = new THREE.Mesh(
      new THREE.BoxGeometry(6.5, 4.22, 0.12),
      new THREE.MeshStandardMaterial({ color: 0x17201e, roughness: 0.42, metalness: 0.45 }),
    )
    const screen = new THREE.Mesh(
      new THREE.PlaneGeometry(6.28, 4.04),
      new THREE.MeshBasicMaterial({ map: screenTexture, toneMapped: false }),
    )
    screen.position.z = 0.071
    const screenGroup = new THREE.Group()
    screenGroup.add(screenFrame, screen)
    screenGroup.position.set(2.7, -0.12, 0)
    screenGroup.rotation.set(-0.04, -0.2, 0.015)
    world.add(screenGroup)

    const waveforms: THREE.Line[] = waveformColors.map((color, lineIndex) => {
      const geometry = new THREE.BufferGeometry()
      const points = new Float32Array(240 * 3)
      geometry.setAttribute('position', new THREE.BufferAttribute(points, 3))
      const material = new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.72 - lineIndex * 0.12 })
      const line = new THREE.Line(geometry, material)
      line.position.set(-2.5, 0, -0.25 - lineIndex * 0.34)
      world.add(line)
      return line
    })

    const particlePositions = new Float32Array(420 * 3)
    for (let index = 0; index < particlePositions.length; index += 3) {
      const seed = index / 3
      particlePositions[index] = ((seed * 47.13) % 13) - 6.5
      particlePositions[index + 1] = ((seed * 31.71) % 9) - 4.5
      particlePositions[index + 2] = ((seed * 19.37) % 9) - 5.5
    }
    const particleGeometry = new THREE.BufferGeometry()
    particleGeometry.setAttribute('position', new THREE.BufferAttribute(particlePositions, 3))
    const particles = new THREE.Points(
      particleGeometry,
      new THREE.PointsMaterial({ color: 0x8dd8c5, size: 0.025, transparent: true, opacity: 0.5 }),
    )
    world.add(particles)

    const die = new THREE.Mesh(
      new THREE.IcosahedronGeometry(0.74, 0),
      new THREE.MeshBasicMaterial({ color: 0xffc857, wireframe: true, transparent: true, opacity: 0.7 }),
    )
    die.position.set(5.2, 3.15, -0.6)
    world.add(die)

    const papers = Array.from({ length: 3 }, (_, index) => {
      const paper = new THREE.Mesh(
        new THREE.PlaneGeometry(1.18, 1.54, 8, 8),
        new THREE.MeshStandardMaterial({
          color: index === 1 ? 0xffdf7d : 0xe6eee8,
          roughness: 0.88,
          side: THREE.DoubleSide,
          transparent: true,
          opacity: 0.92,
        }),
      )
      paper.position.set(4.6 + index * 0.18, -2.7 + index * 0.12, -1.2 - index * 0.3)
      paper.rotation.set(-0.2, 0.35, -0.1 + index * 0.08)
      world.add(paper)
      return paper
    })

    let scrollProgress = 0
    let pointerX = 0
    let pointerY = 0
    let animationFrame = 0

    const updateScroll = () => {
      const journey = document.querySelector<HTMLElement>('[data-scene-journey]')
      const end = journey ? journey.offsetTop + journey.offsetHeight : window.innerHeight * 3
      scrollProgress = THREE.MathUtils.clamp(window.scrollY / Math.max(end - window.innerHeight, 1), 0, 1)
    }

    const updatePointer = (event: PointerEvent) => {
      pointerX = (event.clientX / window.innerWidth - 0.5) * 2
      pointerY = (event.clientY / window.innerHeight - 0.5) * 2
    }

    const resize = () => {
      const width = host.clientWidth
      const height = host.clientHeight
      renderer.setSize(width, height, false)
      camera.aspect = width / Math.max(height, 1)
      camera.updateProjectionMatrix()
      const mobile = width < 760
      screenGroup.scale.setScalar(mobile ? 0.7 : 1)
      screenGroup.position.x = mobile ? 1.35 : 2.7
      screenGroup.position.y = mobile ? -1.35 : -0.12
      die.position.x = mobile ? 3 : 5.2
    }

    const render = (time: number) => {
      const seconds = time * 0.001
      const motion = reducedMotion ? 0 : seconds
      const pointerWeight = reducedMotion ? 0 : 1

      waveforms.forEach((line, lineIndex) => {
        const position = line.geometry.attributes.position as THREE.BufferAttribute
        for (let pointIndex = 0; pointIndex < position.count; pointIndex += 1) {
          const ratio = pointIndex / (position.count - 1)
          const x = ratio * 12 - 6
          const envelope = Math.sin(ratio * Math.PI)
          const frequency = 11 + lineIndex * 3 + scrollProgress * 12
          const amplitude = (0.28 + scrollProgress * 0.42) * envelope
          position.setXYZ(
            pointIndex,
            x,
            Math.sin(ratio * frequency + motion * (1.25 + lineIndex * 0.22)) * amplitude + (lineIndex - 1) * 0.22,
            Math.cos(ratio * 8 + motion * 0.45) * 0.18,
          )
        }
        position.needsUpdate = true
        line.rotation.z = -0.08 + scrollProgress * 0.2
        line.position.y = 0.7 - scrollProgress * 2.1
      })

      screenGroup.rotation.y = -0.2 + pointerX * 0.025 * pointerWeight + scrollProgress * 0.24
      screenGroup.rotation.x = -0.04 - pointerY * 0.018 * pointerWeight - scrollProgress * 0.05
      screenGroup.position.y += ((-0.12 + scrollProgress * 0.45) - screenGroup.position.y) * 0.035
      screenGroup.position.z = scrollProgress * -1.2
      screenGroup.scale.multiplyScalar(1 + Math.sin(motion * 0.55) * 0.0002)

      papers.forEach((paper, index) => {
        const spread = THREE.MathUtils.smoothstep(scrollProgress, 0.28, 0.78)
        paper.position.x = 4.45 - spread * (2.2 + index * 1.05)
        paper.position.y = -2.65 + spread * (2.4 + (index % 2) * 1.3)
        paper.position.z = -1.1 + spread * (0.5 + index * 0.2)
        paper.rotation.z = -0.12 + index * 0.1 + spread * (-0.25 + index * 0.28)
        paper.rotation.y = 0.35 - spread * 0.42
      })

      die.rotation.x = motion * 0.24 + scrollProgress * Math.PI
      die.rotation.y = motion * 0.36 + scrollProgress * Math.PI * 1.4
      particles.rotation.y = motion * 0.012 + scrollProgress * 0.12
      world.rotation.y += ((pointerX * 0.018 * pointerWeight) - world.rotation.y) * 0.035
      world.rotation.x += ((pointerY * -0.012 * pointerWeight) - world.rotation.x) * 0.035

      renderer.render(scene, camera)
      animationFrame = window.requestAnimationFrame(render)
    }

    updateScroll()
    resize()
    window.addEventListener('resize', resize)
    window.addEventListener('scroll', updateScroll, { passive: true })
    window.addEventListener('pointermove', updatePointer, { passive: true })
    animationFrame = window.requestAnimationFrame(render)

    return () => {
      window.cancelAnimationFrame(animationFrame)
      window.removeEventListener('resize', resize)
      window.removeEventListener('scroll', updateScroll)
      window.removeEventListener('pointermove', updatePointer)
      waveforms.forEach((line) => {
        line.geometry.dispose()
        ;(line.material as THREE.Material).dispose()
      })
      papers.forEach((paper) => {
        paper.geometry.dispose()
        ;(paper.material as THREE.Material).dispose()
      })
      screen.geometry.dispose()
      ;(screen.material as THREE.Material).dispose()
      screenFrame.geometry.dispose()
      ;(screenFrame.material as THREE.Material).dispose()
      die.geometry.dispose()
      ;(die.material as THREE.Material).dispose()
      particleGeometry.dispose()
      ;(particles.material as THREE.Material).dispose()
      screenTexture.dispose()
      renderer.dispose()
      renderer.domElement.remove()
    }
  }, [screenshotUrl])

  return <div className="scene-canvas" ref={hostRef} aria-hidden="true" />
}
