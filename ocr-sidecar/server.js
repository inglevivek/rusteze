const express = require('express');
const cors = require('cors');
const Lens = require('chrome-lens-ocr');
const multer = require('multer');

const app = express();
app.use(cors());
const upload = multer({ dest: 'uploads/' });

// Initialize the Lens OCR instance
const lens = new Lens.default();

app.post('/extract', upload.single('document'), async (req, res) => {
    try {
        if (!req.file) {
            return res.status(400).json({ error: "No document provided" });
        }

        console.log(`[OCR] Processing: ${req.file.path}`);
        const result = await lens.scanByFile(req.file.path);
        
        // Ensure we have a combined text string
        const text = result.text || result.segments.map(s => s.text).join('\n');

        // Clean up the uploaded file to save space
        const fs = require('fs');
        fs.unlinkSync(req.file.path);

        res.json({ text: text, segments: result.segments });
    } catch (error) {
        console.error("[OCR] Detailed Error:", error);
        res.status(500).json({ 
            error: error.message,
            stack: error.stack
        });
    }
});

const PORT = 3001;
app.listen(PORT, () => {
    console.log(`OCR Sidecar listening on port ${PORT}`);
});