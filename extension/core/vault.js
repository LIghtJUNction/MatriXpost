import { AppError, assert } from './errors.js';

const enc = new TextEncoder();
const dec = new TextDecoder();
const ITERATIONS = 600000;
const toBase64 = bytes => btoa(String.fromCharCode(...new Uint8Array(bytes)));
const fromBase64 = value => Uint8Array.from(atob(value), c => c.charCodeAt(0));

export class MemorySession {
  constructor() { this.data = {}; }
  async get(key) { return { [key]: this.data[key] }; }
  async set(obj) { Object.assign(this.data,obj); }
  async remove(key) { delete this.data[key]; }
}

export class Vault {
  constructor(store,session=new MemorySession()) { this.store=store; this.session=session; }
  async status() {
    const config=await this.store.get('settings','vault');
    const session=await this.session.get('matrixpostKey');
    return {configured:!!config,unlocked:!!config && !!session.matrixpostKey};
  }
  async derive(passphrase,salt) {
    assert(typeof passphrase==='string' && passphrase.length>=12 && passphrase.length<=1024,
      'INVALID_PASSPHRASE','口令需要 12–1024 个字符。');
    const material=await crypto.subtle.importKey('raw',enc.encode(passphrase),'PBKDF2',false,['deriveKey']);
    return crypto.subtle.deriveKey({name:'PBKDF2',salt:fromBase64(salt),iterations:ITERATIONS,hash:'SHA-256'},
      material,{name:'AES-GCM',length:256},true,['encrypt','decrypt']);
  }
  async sealWith(key,value,identity) {
    const iv=crypto.getRandomValues(new Uint8Array(12));
    const data=await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:enc.encode('matrixpost:v1:'+identity)},key,enc.encode(JSON.stringify(value)));
    return {version:1,iv:toBase64(iv),data:toBase64(data)};
  }
  async openWith(key,envelope,identity) {
    assert(envelope?.version===1,'VAULT_FORMAT','凭证格式无法读取。');
    const data=await crypto.subtle.decrypt({name:'AES-GCM',iv:fromBase64(envelope.iv),additionalData:enc.encode('matrixpost:v1:'+identity)},key,fromBase64(envelope.data));
    return JSON.parse(dec.decode(data));
  }
  async setup(passphrase) {
    assert(!(await this.store.get('settings','vault')),'VAULT_EXISTS','保险库已创建，请解锁。');
    const salt=toBase64(crypto.getRandomValues(new Uint8Array(16)));
    const key=await this.derive(passphrase,salt);
    const verifier=await this.sealWith(key,{check:'matrixpost-v1'},'verifier');
    await this.store.put('settings',{id:'vault',salt,iterations:ITERATIONS,verifier});
    await this.remember(key);
    return this.status();
  }
  async remember(key) {
    await this.session.set({matrixpostKey:toBase64(await crypto.subtle.exportKey('raw',key))});
  }
  async key() {
    const {matrixpostKey}=await this.session.get('matrixpostKey');
    assert(matrixpostKey,'VAULT_LOCKED','请先解锁账号保险库。');
    return crypto.subtle.importKey('raw',fromBase64(matrixpostKey),{name:'AES-GCM'},false,['encrypt','decrypt']);
  }
  async unlock(passphrase) {
    const config=await this.store.get('settings','vault');
    assert(config,'VAULT_NOT_CONFIGURED','请先设置账号保险库口令。');
    try {
      const key=await this.derive(passphrase,config.salt);
      const value=await this.openWith(key,config.verifier,'verifier');
      assert(value.check==='matrixpost-v1','VAULT_UNLOCK_FAILED','口令错误或保险库已损坏。');
      await this.remember(key);
    } catch { throw new AppError('VAULT_UNLOCK_FAILED','口令错误或保险库已损坏。'); }
    return this.status();
  }
  async lock() { await this.session.remove('matrixpostKey'); return this.status(); }
  async seal(value,identity) { return this.sealWith(await this.key(),value,identity); }
  async open(envelope,identity) {
    try { return await this.openWith(await this.key(),envelope,identity); }
    catch(error) {
      if(error.code==='VAULT_LOCKED') throw error;
      throw new AppError('VAULT_DECRYPT_FAILED','凭证无法解密，请重新保存该账号的授权。');
    }
  }
}
