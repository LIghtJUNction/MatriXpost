const STORES = ['drafts', 'accounts', 'jobs', 'assets', 'settings', 'secrets'];

export class IndexedStore {
  constructor(name = 'matrixpost-v1') { this.name = name; this.opening = null; }
  async db() {
    if (!this.opening) this.opening = new Promise((resolve, reject) => {
      const req = indexedDB.open(this.name, 1);
      req.onupgradeneeded = () => {
        for (const name of STORES) if (!req.result.objectStoreNames.contains(name)) {
          req.result.createObjectStore(name, { keyPath: 'id' });
        }
      };
      req.onsuccess = () => {
        req.result.onversionchange = () => { req.result.close(); this.opening = null; };
        resolve(req.result);
      };
      req.onerror = () => { this.opening = null; reject(req.error); };
      req.onblocked = () => { this.opening = null; reject(new Error('Database blocked')); };
    });
    return this.opening;
  }
  async operation(store, mode, fn) {
    const db = await this.db();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(store, mode);
      const req = fn(tx.objectStore(store));
      let result;
      req.onsuccess = () => { result = req.result; };
      tx.oncomplete = () => resolve(result);
      tx.onerror = () => reject(tx.error || req.error);
      tx.onabort = () => reject(tx.error || new Error('Transaction aborted'));
    });
  }
  get(store,id) { return this.operation(store, 'readonly', s => s.get(id)); }
  all(store) { return this.operation(store, 'readonly', s => s.getAll()); }
  put(store,value) { return this.operation(store, 'readwrite', s => s.put(value)).then(() => value); }
  delete(store,id) { return this.operation(store, 'readwrite', s => s.delete(id)); }
  async batch(operations) {
    if (!operations.length) return;
    const db = await this.db();
    return new Promise((resolve,reject) => {
      const tx=db.transaction([...new Set(operations.map(o=>o.store))], 'readwrite');
      for (const op of operations) {
        const s=tx.objectStore(op.store);
        if (op.delete) s.delete(op.id); else s.put(op.value);
      }
      tx.oncomplete=()=>resolve(); tx.onerror=()=>reject(tx.error); tx.onabort=()=>reject(tx.error);
    });
  }
}

export class MemoryStore {
  constructor() { this.data=Object.fromEntries(STORES.map(s=>[s,new Map()])); }
  async get(store,id) { return structuredClone(this.data[store].get(id)); }
  async all(store) { return structuredClone([...this.data[store].values()]); }
  async put(store,value) { this.data[store].set(value.id,structuredClone(value)); return structuredClone(value); }
  async delete(store,id) { this.data[store].delete(id); }
  async batch(operations) {
    for(const op of operations) { if(op.delete) await this.delete(op.store,op.id); else await this.put(op.store,op.value); }
  }
}
