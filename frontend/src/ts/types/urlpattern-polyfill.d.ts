// The polyfill is only imported for its side effect (see cup-app.ts). Its own
// typings declare a global URLPattern, which clashes with the one in lib.dom
// since TypeScript 6, so type checking resolves the module to this stub instead
// (tsconfig "paths"; the bundler still uses the real package).
export {};
