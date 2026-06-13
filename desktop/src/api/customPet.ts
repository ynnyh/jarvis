import { invoke } from '@tauri-apps/api/core'

export type CustomPetType = 'lottie' | 'image' | 'gif'
export type ImageAnimation = 'breath' | 'swing' | 'rotate' | 'bounce' | 'none'

export interface CustomPetMeta {
  id: string
  name: string
  description: string
  type: CustomPetType
  animation?: ImageAnimation
}

export interface CustomPet extends CustomPetMeta {
  data: unknown
}

export async function customPetList(): Promise<CustomPet[]> {
  return invoke<CustomPet[]>('custom_pet_list')
}

export async function customPetSave(pet: CustomPet): Promise<void> {
  return invoke('custom_pet_save', { pet })
}

export async function customPetDelete(id: string): Promise<void> {
  return invoke('custom_pet_delete', { id })
}

export function generateCustomPetId(): string {
  return `custom-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}
