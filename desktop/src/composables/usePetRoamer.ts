// ============================================================================
// E2 漫游行为引擎：让 pack 宠物在桌面上「活」起来。
//
// 只对「当前宠物是 pack 且 petRoaming 开启」的场景生效；lottie/图片宠物与
// petRoaming=false 时零行为变化（active=false 即完全不排定时器）。
//
// 决策循环：idle 下每 15-40s 随机决策一次 —— 45% 继续 idle（方案 A 的 fidget
// 照旧）、35% 播加权随机小动作、20% 走路（无 walk clip 退化为小动作）。
//
// 优先级与打断（高→低，PRD E2-3）：
//   拖拽 > 气泡在场 > agent 活动 > dock > 决策循环
// 每种打断路径对走路 rAF / clip 的处理：
//   - 拖拽（isDragging 置位）：rAF 立即取消 + walk clip 收尾 + 播 held（若有）；
//     dragEnd 收掉 held 回 idle。
//   - 气泡在场（hasAlert）：立即停走冻结当前位置（PRD E2-3「暂停走动」）——
//     showAlert 触发的 ensureBubbleVisible 会 animateWindowToLogical 矫正窗口
//     位置，与走路 rAF 并发写 setPosition 互相打架，且走路闭包里的旧坐标会把
//     矫正结果冲掉（气泡被推回屏外）；之后不再发起新决策，气泡消失后自愈续跑。
//   - agent 活动：不发起走路（发起时检查），在走的走完当前目标即停。
//   - dock（dockEdge 非空）：rAF 立即取消（dock 的 animateWindowToLogical 要
//     独占窗口移动权）+ walk clip 收尾，决策挂起；exitDock 恢复排期。
//
// 走路几何：小人恒居 900×900 窗口正中心（ANCHOR_AVATAR_CENTER 四键恒等），
// 脚 = 渲染盒底缘 = 窗口中心 + petSizePx/2。目标 = 当前 x ± 100~400px，clamp 在
// 当前显示器「工作区」内；y 一次算定为「脚踩工作区底边」（任务栏上缘），走动
// 途中 y 以同速向底边靠拢（不在起点瞬移贴地，从半空被拖上去的场景是"走下去"）。
// 移动用自写 rAF 线性匀速步进（~60px/s），刻意不用 dock 的缓动 —— 漫游要匀速
// 可取消，缓动是奔固定点的收束动画，语义不同。
//
// 睡觉：连续无交互 ≥10 分钟且有 sleep clip → 循环播 sleep；任何点击/拖拽/
// 气泡/菜单交互唤醒回 idle。交互时钟由 notifyInteraction（App.vue mousedown）+
// 各信号 watcher（拖拽/气泡/菜单/dock/走路启动）共同 bump。
// ============================================================================

import { ref, computed, watch, onUnmounted, type Ref } from 'vue'
import { getCurrentWindow, currentMonitor, LogicalPosition } from '@tauri-apps/api/window'
import { useConfigStore } from '../stores/config'
import { isPackPetId, packHasClip, getPackPets } from '../petManifest'
import { ANCHOR_AVATAR_CENTER, type DockEdge } from './useAvatarDock'

/** PetAvatar defineExpose 的结构化口径（避免 composable 直接依赖组件类型）。
 *  注意 currentClip 是「解包后」的值：defineExpose 暴露的对象经 proxyRefs 包装，
 *  组件 ref 上读到的 currentClip 是 string|null 本体而非 Ref —— 按 expose 语义声明。 */
export interface PetAvatarApi {
  playClip: (name: string, opts?: { loop?: boolean }) => Promise<void>
  stopClip: () => void
  hasClip: (name: string) => boolean
  setFacing: (dir: 'left' | 'right') => void
  currentClip: string | null
}

export interface UsePetRoamerOptions {
  /** PetAvatar 组件 ref（orchestration 持有，App.vue 模板绑定） */
  petAvatar: Ref<PetAvatarApi | null>
  /** 拖拽进行中（useAvatarDrag）——最高优先级打断源 */
  isDragging: Ref<boolean>
  /** 气泡在场（orchestration hasAlert）——暂停发起新决策 */
  hasAlert: Ref<boolean>
  /** 右键菜单开着 —— 菜单交互期不发起（菜单交互本身也唤醒睡觉） */
  showMenu: Ref<boolean>
  /** dock 收纳边（null = 未 dock）——入 dock 挂起，出 dock 恢复 */
  dockEdge: Ref<DockEdge | null>
  /** agent 会话进行中（chat:stream 生命周期）——期间不发起走路 */
  isAgentActive: () => boolean
  /** 面板/迷你对话开着（工具窗会藏小人、面板贴着小人）——不发起 */
  isPanelOpen: () => boolean
}

// ===== 常量（PRD E2 口径）=====
/** 决策间隔：15-40s 随机 */
const DECIDE_MIN_MS = 15000
const DECIDE_MAX_MS = 40000
/** 走路速度 ~60px/s（逻辑像素），PRD 可配常量 */
const WALK_SPEED_PX_S = 60
/** 单次走路距离：100~400px */
const WALK_MIN_PX = 100
const WALK_MAX_PX = 400
/** 连续无交互 ≥10 分钟且 pack 有 sleep → 睡觉 */
const SLEEP_AFTER_IDLE_MS = 10 * 60 * 1000
/** 睡觉判定的巡检间隔 */
const SLEEP_CHECK_INTERVAL_MS = 15000

/** 当前显示器工作区（逻辑像素全局坐标）。dock 的 getMonitorBounds 是全屏口径，
 *  走路要贴「任务栏上缘」必须用 workArea —— Tauri v2 Monitor 自带 workArea，
 *  照 getMonitorBounds 的逻辑坐标换算先例除以 scaleFactor。 */
async function getWorkAreaBounds(): Promise<{ x: number; y: number; w: number; h: number }> {
  const mon = await currentMonitor()
  if (!mon) return { x: 0, y: 0, w: window.screen.availWidth, h: window.screen.availHeight }
  const s = mon.scaleFactor
  return {
    x: mon.workArea.position.x / s,
    y: mon.workArea.position.y / s,
    w: mon.workArea.size.width / s,
    h: mon.workArea.size.height / s,
  }
}

export function usePetRoamer(options: UsePetRoamerOptions) {
  const { petAvatar, isDragging, hasAlert, showMenu, dockEdge } = options
  const configStore = useConfigStore()

  /** 窗口移动进行中：暴露给 squish/fidget 门控（模式照 isDragging） */
  const isMoving = ref(false)
  /** 睡觉中（sleep clip 循环播放） */
  let sleeping = false
  /** 最近一次交互时刻（睡觉判定基准） */
  let lastInteractAt = Date.now()
  /** 拖拽期由我们播的 held clip（dragEnd 时收尾；不碰用户点播的片段） */
  let playingHeld = false

  // 小人渲染盒半径（与 useAvatarDock 的 avatarHalf 同源：petSizePx/2，加载时已钳制）
  const avatarHalf = () => configStore.config.petSizePx / 2
  // 窗口中心 = 小人中心（四键恒等，取 rb 即可）
  const CENTER = ANCHOR_AVATAR_CENTER.rb

  /** 当前宠物的包清单（非 pack 宠物返回 null） */
  function currentPack() {
    const petId = configStore.config.petId
    if (!isPackPetId(petId)) return null
    return getPackPets().find(p => p.id === petId)?.pack ?? null
  }

  /** 引擎总闸：config 已加载 + 开关开启 + 当前宠物是 pack（idle 必有）。 */
  const active = computed(() =>
    configStore.loaded
    && configStore.config.petRoaming !== false
    && packHasClip(configStore.config.petId, 'idle'),
  )

  // ===== 定时器 / 走路会话（onUnmounted 全清理）=====

  let decideTimer: number | null = null
  let sleepTimer: number | null = null

  interface WalkSession {
    raf: number | null
    finished: boolean
    resolve: () => void
  }
  let walk: WalkSession | null = null

  function clearDecideTimer() {
    if (decideTimer !== null) { clearTimeout(decideTimer); decideTimer = null }
  }

  /** 排下一轮决策（幂等：先清旧 timer，防打断路径重复排期叠加） */
  function scheduleDecision(delayMs = DECIDE_MIN_MS + Math.random() * (DECIDE_MAX_MS - DECIDE_MIN_MS)) {
    if (!active.value) return
    clearDecideTimer()
    decideTimer = window.setTimeout(decide, delayMs)
  }

  /** 立即收掉「我们自己播的」某个 clip；用户点播中的片段不碰 */
  function stopOwnClip(name: string) {
    const avatar = petAvatar.value
    if (avatar && avatar.currentClip === name) avatar.stopClip()
  }

  // ===== 走路 =====

  /** 打断走路：rAF 立即取消 + resolve 挂起的 startWalk promise。幂等。 */
  function stopWalking() {
    if (!walk) return
    const session = walk
    walk = null
    isMoving.value = false
    if (session.raf !== null) cancelAnimationFrame(session.raf)
    session.finished = true
    session.resolve()
  }

  /**
   * 走一段：目标 x = 当前 ±100~400px（clamp 在当前显示器工作区内），y 贴工作区
   * 底边（脚踩任务栏上缘）。播 walk clip（loop）+ 按行进方向翻转素材。到达或被
   * 打断后 resolve（调用方决定后续排期）；walk clip 的收尾统一在完成后做（被打
   * 断时由打断方按优先级接管 clip：held / dock 静默回 idle）。
   */
  async function startWalk(): Promise<void> {
    const avatar = petAvatar.value
    if (!avatar || avatar.currentClip !== null) return
    const win = getCurrentWindow()
    let winX: number, winY: number
    try {
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      winX = pos.x / scale
      winY = pos.y / scale
    } catch {
      return
    }
    const area = await getWorkAreaBounds()
    const half = avatarHalf()
    // 异步取坐标/工作区期间可能已被打断（拖拽/dock 抢先），重查一遍再动窗口
    if (walk !== null || isDragging.value || dockEdge.value) return

    const centerX = winX + CENTER.x
    // 小人渲染盒全须留在工作区内：中心 clamp 到 [左缘+half, 右缘-half]
    const minX = area.x + half
    const maxX = area.x + area.w - half
    const dist = WALK_MIN_PX + Math.random() * (WALK_MAX_PX - WALK_MIN_PX)
    let dir: 1 | -1 = Math.random() < 0.5 ? -1 : 1
    let targetCenterX = centerX + dir * dist
    if (targetCenterX < minX || targetCenterX > maxX) {
      // 该方向出界 → 反方向走（贴边场景变成「往空处走满 100~400px」）
      dir = dir === 1 ? -1 : 1
      targetCenterX = Math.min(Math.max(centerX + dir * dist, minX), maxX)
    } else {
      targetCenterX = Math.min(Math.max(targetCenterX, minX), maxX)
    }
    // 两侧都顶死（工作区比行程还窄，正常屏幕不会发生）：放弃本次
    if (Math.abs(targetCenterX - centerX) < WALK_MIN_PX) return

    // 脚踩工作区底边：底边 - 渲染盒半径 = 小人中心 y → 反推窗口 y
    const targetWinX = targetCenterX - CENTER.x
    const targetWinY = area.y + area.h - half - CENTER.y

    // 按行进方向翻素材（素材层 pointer-events:none，翻转不碰命中盒）
    avatar.setFacing(dir === 1 ? 'right' : 'left')
    isMoving.value = true
    // loop clip 的 promise 永不 resolve（靠 stopClip 收尾），fire-and-forget 即可
    void avatar.playClip('walk', { loop: true })

    const session: WalkSession = { raf: null, finished: false, resolve: () => {} }
    walk = session
    await new Promise<void>((resolve) => {
      session.resolve = resolve
      // 闭包持有当前逻辑坐标逐帧推进：窗口 setPosition 是异步 IPC，逐帧回读
      // outerPosition 会拿到滞后值造成步进抖动，参照 dock 动画的「起点+推进」写法
      let curX = winX
      let curY = winY
      let last = performance.now()
      const step = (now: number) => {
        if (session.finished || walk !== session) return
        // 长帧钳制：窗口隐藏期 rAF 暂停，恢复瞬间 dt 巨大，不钳会瞬移
        const dt = Math.min((now - last) / 1000, 0.1)
        last = now
        const stepLen = WALK_SPEED_PX_S * dt
        const dx = targetWinX - curX
        const dy = targetWinY - curY
        const arrive = Math.abs(dx) <= stepLen && Math.abs(dy) <= stepLen
        if (arrive) {
          curX = targetWinX
          curY = targetWinY
        } else {
          curX += Math.sign(dx) * Math.min(Math.abs(dx), stepLen)
          curY += Math.sign(dy) * Math.min(Math.abs(dy), stepLen)
        }
        win.setPosition(new LogicalPosition(Math.round(curX), Math.round(curY))).catch(() => {})
        if (arrive) {
          walk = null
          session.finished = true
          isMoving.value = false
          resolve()
          return
        }
        session.raf = requestAnimationFrame(step)
      }
      session.raf = requestAnimationFrame(step)
    })
    // 到达（或打断后由本函数收尾的场景）：walk clip 收尾回 idle。
    // 只收自己播的 walk —— 打断方（拖拽）可能已经换成 held 了。
    stopOwnClip('walk')
  }

  // ===== 小动作（weight 加权随机）=====

  /** 播一个加权随机小动作；播完自动回 idle 后续排决策。没有可用动作返回 false。 */
  function playRandomAction(avatar: PetAvatarApi): boolean {
    const actions = currentPack()?.clips.actions ?? []
    if (actions.length === 0) return false
    const weightOf = (w?: number) => Math.max(0, w ?? 1)
    const total = actions.reduce((sum, a) => sum + weightOf(a.weight), 0)
    if (total <= 0) return false
    let roll = Math.random() * total
    for (const action of actions) {
      roll -= weightOf(action.weight)
      if (roll < 0) {
        // 包一层箭头：scheduleDecision 的可选参数不能直接当 promise 回调（TS 签名不合）
        void avatar.playClip(action.name).then(() => scheduleDecision())
        return true
      }
    }
    return false
  }

  // ===== 决策循环 =====

  function decide() {
    decideTimer = null
    if (!active.value || sleeping) return
    // 发起前资格检查（高优先级打断源在场就让路）：不满足本轮跳过、重排再试。
    // 事件驱动的恢复（气泡消失/dock 退出）另有 watcher 兜底，这里自愈即可。
    if (
      isDragging.value
      || hasAlert.value
      || showMenu.value
      || dockEdge.value
      || options.isAgentActive()
      || options.isPanelOpen()
    ) {
      scheduleDecision()
      return
    }
    const avatar = petAvatar.value
    // 用户点播片段在播（「来一个」菜单）：不抢，重排
    if (!avatar || avatar.currentClip !== null) {
      scheduleDecision()
      return
    }
    const roll = Math.random()
    if (roll < 0.45) {
      // 45% 继续 idle：方案 A 的 fidget 微动作照常自己跑
      scheduleDecision()
      return
    }
    if (roll < 0.8) {
      // 35% 播随机小动作（pack 有 actions 时；没有就继续 idle）
      if (playRandomAction(avatar)) return
      scheduleDecision()
      return
    }
    // 20% 走路：agent 活动期间不发起走路（PRD E2-3）；无 walk clip 退化为小动作
    if (packHasClip(configStore.config.petId, 'walk') && !options.isAgentActive()) {
      void startWalk().then(() => scheduleDecision())
      return
    }
    if (playRandomAction(avatar)) return
    scheduleDecision()
  }

  // ===== 睡觉 =====

  function bumpActivity() {
    lastInteractAt = Date.now()
  }

  /** 唤醒：任何点击/拖拽/气泡/菜单交互（睡梦中才需要收 sleep clip）。
   *  唤醒即恢复决策排期：入睡时 clearDecideTimer 停了摆，而气泡/菜单/点击
   *  这几条唤醒路径没有别的重排钩子 —— 不补排的话循环会一直停到下次
   *  拖拽/dock/换宠。打断源仍在场也无妨，decide() 资格检查会让路重排
   *  （dock watcher 在 wake 之后有 clearDecideTimer，不会被这里排出的 timer 穿透）。 */
  function wake() {
    bumpActivity()
    scheduleDecision()
    if (!sleeping) return
    sleeping = false
    stopOwnClip('sleep')
  }

  function checkSleep() {
    if (!active.value || sleeping) return
    if (isDragging.value || hasAlert.value || showMenu.value || dockEdge.value) {
      // 交互源在场：刷新时钟，攒不够「连续 idle」
      bumpActivity()
      return
    }
    if (options.isAgentActive() || options.isPanelOpen()) return
    const avatar = petAvatar.value
    // 有片段在播（含点播）不算 idle；无 sleep clip 不睡
    if (!avatar || avatar.currentClip !== null) return
    if (!packHasClip(configStore.config.petId, 'sleep')) return
    if (Date.now() - lastInteractAt < SLEEP_AFTER_IDLE_MS) return
    sleeping = true
    clearDecideTimer() // 睡觉期间决策循环停摆，唤醒时重排
    void avatar.playClip('sleep', { loop: true })
  }

  // ===== 交互信号接线（唤醒 + 交互时钟）=====

  // 拖拽（最高优先级）：sync 立即打断 —— Windows 自驱拖拽的 Rust 线程随即接管
  // 窗口移动，走路 rAF 晚停一帧都会和它打架抢 setPosition。
  watch(isDragging, (dragging) => {
    if (dragging) {
      wake()
      if (walk) {
        stopWalking()
        stopOwnClip('walk')
      }
      // 拎起播 held（若有）；用户点播中的片段也让位 —— 拎起语义优先
      const avatar = petAvatar.value
      if (avatar && !sleeping && packHasClip(configStore.config.petId, 'held') && avatar.currentClip !== 'held') {
        playingHeld = true
        void avatar.playClip('held', { loop: true })
      }
    } else {
      // 落手：held 收尾回 idle，恢复决策排期
      if (playingHeld) {
        playingHeld = false
        stopOwnClip('held')
      }
      bumpActivity()
      scheduleDecision()
    }
  }, { flush: 'sync' })

  // 气泡在场：立即冻结走路（PRD E2-3「暂停走动：冻结当前位置」）—— showAlert
  // 触发的 ensureBubbleVisible 会 animateWindowToLogical 矫正窗口位置，与走路
  // rAF 并发写 setPosition 互相打架，且走路闭包里的旧坐标会把矫正结果冲回
  // （气泡可能被顶回屏外）。停走即冻结在当前帧位置；「不再发起新决策」由
  // decide() 资格检查承担，气泡消失后经 stopWalking 的 resolve 链自愈重排
  watch(hasAlert, (present) => {
    if (present) {
      wake()
      if (walk) stopWalking()
    }
  })

  // 菜单交互：唤醒 + 刷时钟；「菜单开着不发起」由 decide() 资格检查承担
  watch(showMenu, (open) => {
    if (open) wake()
  })

  // dock：入 dock 挂起（dock 动画独占窗口移动权，走路 rAF 必须立刻让位），
  // 出 dock 恢复决策排期
  watch(dockEdge, (edge) => {
    if (edge) {
      wake()
      if (walk) {
        stopWalking()
        stopOwnClip('walk')
      }
      clearDecideTimer()
    } else {
      bumpActivity()
      scheduleDecision()
    }
  }, { flush: 'sync' })

  // 换宠物：走到一半换形象 —— 立即停走（PetAvatar 的 pack watcher 已收片段），
  // 新宠物若是 pack 则重开决策，交互时钟重置
  watch(() => configStore.config.petId, () => {
    stopWalking()
    playingHeld = false
    if (sleeping) {
      sleeping = false
      stopOwnClip('sleep')
    }
    bumpActivity()
    clearDecideTimer()
    if (active.value) scheduleDecision()
  })

  // 总闸：开启 = 起决策循环 + 起睡觉巡检；关闭 = 全部停掉（与旧版本行为一致）
  watch(active, (on) => {
    if (on) {
      bumpActivity()
      if (!sleepTimer) sleepTimer = window.setInterval(checkSleep, SLEEP_CHECK_INTERVAL_MS)
      scheduleDecision()
    } else {
      clearDecideTimer()
      if (sleepTimer) { clearInterval(sleepTimer); sleepTimer = null }
      stopWalking()
      // 三个自播 clip（walk/held/sleep）全收：引擎下线 = 小人回 idle 常态。
      // held 场景（roaming 在拖拽中途被关）若不收会永远循环。
      stopOwnClip('walk')
      stopOwnClip('held')
      if (sleeping) { sleeping = false; stopOwnClip('sleep') }
      playingHeld = false
    }
  }, { immediate: true })

  // App.vue 的 mousedown 直通（点击小人 = 交互；面板/菜单开合各有 watcher 兜底）
  function notifyInteraction() {
    wake()
  }

  onUnmounted(() => {
    clearDecideTimer()
    if (sleepTimer) { clearInterval(sleepTimer); sleepTimer = null }
    if (walk) {
      // 只撤 rAF，不 resolve：卸载后无消费者等待，防止 resolve 回调再排定时器
      if (walk.raf !== null) cancelAnimationFrame(walk.raf)
      walk.finished = true
      walk = null
    }
    isMoving.value = false
  })

  return {
    isMoving,
    notifyInteraction,
  }
}
