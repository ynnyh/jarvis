import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import vue from 'eslint-plugin-vue'
import vueParser from 'vue-eslint-parser'
import globals from 'globals'

export default tseslint.config(
  // ===== 忽略目录 =====
  {
    ignores: [
      'dist/**',
      'node_modules/**',
      'src-tauri/**',
      'docs/.vitepress/**',
      'docs/public/**',
      'tools/**',
      'scripts/ci/**',
    ],
  },

  // ===== 基础 JS 推荐 =====
  js.configs.recommended,

  // ===== TypeScript 推荐（带类型检查的规则关掉，避免需要 project 配置） =====
  ...tseslint.configs.recommended,

  // ===== Vue 推荐 =====
  ...vue.configs['flat/recommended'],

  // ===== 项目通用规则 =====
  {
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
      globals: {
        ...globals.browser,
        ...globals.node,
      },
      parser: vueParser,
      parserOptions: {
        parser: tseslint.parser,
        ecmaVersion: 2022,
        sourceType: 'module',
        extraFileExtensions: ['.vue'],
      },
    },
    rules: {
      // —— 渐进式收紧：先 warn，避免一次性阻断 CI ——
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/no-unused-vars': [
        'warn',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
      'no-console': ['warn', { allow: ['warn', 'error'] }],
      'no-debugger': 'warn',
      // 空 catch 块降为 warn：事件监听 .catch(()=>{})、可选链等场景是合法的故意忽略
      'no-empty': ['warn', { allowEmptyCatch: true }],

      // —— Vue 相关 ——
      'vue/multi-word-component-names': 'off', // 单页面窗口组件允许单词命名
      'vue/no-v-html': 'off', // 受信内容（更新日志渲染）

      // —— 关闭纯格式类规则（交给编辑器/Prettier，避免与现有代码风格打架）——
      'vue/html-indent': 'off',
      'vue/script-indent': 'off',
      'vue/max-attributes-per-line': 'off',
      'vue/singleline-html-element-content-newline': 'off',
      'vue/multiline-html-element-content-newline': 'off',
      'vue/html-self-closing': 'off',
      'vue/html-closing-bracket-newline': 'off',
      'vue/attributes-order': 'off',
      'vue/order-in-components': 'off', // 可选开启，但现有组件顺序不一，暂关
      'vue/first-attribute-linebreak': 'off',
    },
  },

  // ===== 脚本/工具文件放宽（CLI 脚本用 console 输出是合理的） =====
  {
    files: [
      'scripts/**/*.mjs',
      'scripts/**/*.js',
      'scripts/**/*.ts',
      'tools/**/*.mjs',
      'tools/**/*.js',
    ],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      'no-console': 'off',
    },
  },

  // ===== mock 文件放宽（模拟数据，console/any 用于调试和简化类型） =====
  {
    files: ['**/*.mock.ts', '**/*.mock.vue', '**/mocks/**'],
    rules: {
      'no-console': 'off',
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },

  // ===== 配置文件本身不 lint 类型规则 =====
  {
    files: ['*.config.ts', '*.config.js', '*.config.mjs', 'eslint.config.js'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },
)
