# Live Gateway Setup — Polymarket CLOB V2

## Prerequisitos antes del 22 de abril

### 1. Wallet Polygon
- Crear wallet nueva dedicada exclusivamente a EXCAMIM
- Fondear con exactamente $50 USDC en Polygon
- Guardar private key en variable de entorno EXCAMIM_PRIVATE_KEY
- NUNCA commitear la private key al repo

### 2. Convertir USDC a pUSD (hacer el 22 abril post-migración)
- Ir a polymarket.com con la wallet
- UI maneja el wrap automáticamente en primera operación
- O llamar wrap() en Collateral Onramp contract manualmente

### 3. API Keys de Polymarket
- Ir a polymarket.com/settings
- Generar API key, secret y passphrase
- Guardar en variables de entorno:
  EXCAMIM_PRIVATE_KEY=
  POLY_API_KEY=
  POLY_SECRET=
  POLY_PASSPHRASE=
  POLY_CHAIN=137

### 4. Builder Code (opcional pero recomendado)
- Ir a polymarket.com/settings?tab=builder
- Registrar el proyecto
- Guardar en POLY_BUILDER_CODE=

## Variables de entorno requeridas
Añadir a /etc/environment en el VPS:
EXCAMIM_PRIVATE_KEY=
POLY_API_KEY=
POLY_SECRET=
POLY_PASSPHRASE=
POLY_CHAIN=137
POLY_BUILDER_CODE=

## Capital inicial
- Máximo $50 en esta fase
- Objetivo: medir fill rate, no ganar dinero
- Tamaño por orden: $5-10
