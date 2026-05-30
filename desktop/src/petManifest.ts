// 宠物形象注册表。每个宠物 = 一个 Lottie JSON + 一组元信息。
//
// 添加新宠物：
//   1. 把 Lottie .json 文件放到 desktop/src/assets/pets/
//   2. 在下方 PETS 数组里加一项，引入新 JSON
//   3. 配置里的 petId 改成新 id 就能切换
//
// 暂用 eager import：全部内置宠物打包进 JS bundle。
// 如果以后内置太多影响加载速度再改 lazy import。

import roboData from './assets/pets/robo.json'
import catMoonData from './assets/pets/cat-moon.json'
import astroLaptopData from './assets/pets/astro-laptop.json'
import astroHeroData from './assets/pets/astro-hero.json'
import happySpacemanData from './assets/pets/Happy Spaceman.json'
import catCryingData from './assets/pets/Cat Crying emojiSticker animation.json'
import cuteTigerData from './assets/pets/Cute Tiger.json'
import slothMeditateData from './assets/pets/Sloth meditate.json'
import cowDrinkMilkData from './assets/pets/Cow Drink Milk.json'
import dancingLlamaData from './assets/pets/Dancing llama.json'

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
  {
    id: 'happy-spaceman',
    name: '开心太空人',
    category: 'character',
    description: '快乐漂浮的太空人',
    data: happySpacemanData,
  },
  {
    id: 'cat-crying',
    name: '哭泣小猫',
    category: 'pet',
    description: '委屈巴巴的哭泣猫咪',
    data: catCryingData,
  },
  {
    id: 'cute-tiger',
    name: '可爱小虎',
    category: 'pet',
    description: '呆萌可爱的小老虎',
    data: cuteTigerData,
  },
  {
    id: 'sloth-meditate',
    name: '冥想树懒',
    category: 'pet',
    description: '闭目冥想的树懒',
    data: slothMeditateData,
  },
  {
    id: 'cow-drink-milk',
    name: '喝奶小牛',
    category: 'pet',
    description: '憨态可掬喝牛奶的小牛',
    data: cowDrinkMilkData,
  },
  {
    id: 'dancing-llama',
    name: '跳舞羊驼',
    category: 'pet',
    description: '魔性舞蹈的羊驼',
    data: dancingLlamaData,
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
