// 宠物形象注册表。每个宠物 = 一个 Lottie JSON + 一组元信息。
//
// 添加新宠物：
//   1. 把 Lottie .json 文件放到 desktop/src/assets/pets/
//   2. 在下方 PETS 数组里加一项，引入新 JSON
//   3. 配置里的 petId 改成新 id 就能切换
//
// 暂用 eager import：每个 JSON ~10-30KB，几个内置宠物加起来不到 200KB，
// 比 lazy import 简单可靠。如果以后内置几十个再改 lazy。

import roboData from './assets/pets/robo.json'
import catMoonData from './assets/pets/cat-moon.json'
import astroLaptopData from './assets/pets/astro-laptop.json'
import astroHeroData from './assets/pets/astro-hero.json'

export type PetCategory = 'mecha' | 'pet' | 'character'

export interface PetInfo {
  id: string
  name: string
  category: PetCategory
  /** 一句话描述，给设置 UI 提示文案用 */
  description: string
  /** Lottie 动画 JSON 数据，直接喂给 lottie.loadAnimation({ animationData }) */
  data: unknown
}

export const PETS: PetInfo[] = [
  {
    id: 'robo',
    name: '小机器人',
    category: 'mecha',
    description: '机甲风默认形象',
    data: roboData,
  },
  {
    id: 'cat-moon',
    name: '月球钓鱼猫',
    category: 'pet',
    description: '在月亮上钓鱼的小猫',
    data: catMoonData,
  },
  {
    id: 'astro-laptop',
    name: '敲键盘宇航员',
    category: 'character',
    description: '抱着笔记本敲代码的小宇航员',
    data: astroLaptopData,
  },
  {
    id: 'astro-hero',
    name: '飞行宇航员',
    category: 'character',
    description: '披风飞行的超级宇航员',
    data: astroHeroData,
  },
]

export const PET_CATEGORY_LABELS: Record<PetCategory, string> = {
  mecha: '机甲',
  pet: '宠物',
  character: '人物',
}

export function getPetById(id: string): PetInfo {
  return PETS.find(p => p.id === id) ?? PETS[0]
}
