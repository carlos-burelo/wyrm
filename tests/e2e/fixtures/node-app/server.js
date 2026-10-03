const http = require('http');

const port = Number(process.env.PORT || 3890);
http
  .createServer((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/plain' });
    res.end('wyrm-e2e ok');
  })
  .listen(port, '127.0.0.1', () => console.log(`e2e fixture en ${port}`));
