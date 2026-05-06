const Lens = require('chrome-lens-ocr');
console.log('Lens type:', typeof Lens);
console.log('Lens keys:', Object.keys(Lens));
if (Lens.default) {
    console.log('Lens.default type:', typeof Lens.default);
    const instance = new Lens.default();
    console.log('Instance type:', typeof instance);
    console.log('scanByFile exists:', typeof instance.scanByFile === 'function');
}

