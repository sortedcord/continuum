// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	site: 'https://sortedcord.github.io',
	base: '/',
	integrations: [
		starlight({
			title: 'Continuum',
			description: 'Apple Continuity-grade integration for Linux + Android',
			social: [
				{
					icon: 'github',
					label: 'GitHub',
					href: 'https://github.com/sortedcord/continuum',
				},
			],
			sidebar: [
				{
					label: 'Overview',
					items: [
						{ label: 'Vision & Philosophy', slug: 'overview/vision' },
						{ label: 'Feature Matrix', slug: 'overview/feature-matrix' },
						{ label: 'Project Roadmap', slug: 'overview/roadmap' },
						{ label: 'Continuum Superpowers', slug: 'overview/superpowers' },
					],
				},
				{
					label: 'Foundational Core',
					items: [
						{ label: 'Ambient Presence & Proximity', slug: 'core/presence' },
						{ label: 'Cryptographic Identity & Trust', slug: 'core/identity' },
					],
				},
				{
					label: 'Content & Data Exchange',
					items: [
						{ label: 'Universal Clipboard', slug: 'content/clipboard' },
						{ label: 'AirDrop File Transfer', slug: 'content/airdrop' },
						{ label: 'Quick Data Sync (Wi-Fi & 2FA)', slug: 'content/quick-sync' },
					],
				},
				{
					label: 'Peripherals & AV Streaming',
					items: [
						{ label: 'Continuity Camera', slug: 'media/camera' },
						{ label: 'Continuity Microphone', slug: 'media/microphone' },
						{ label: 'Media Cast Receiver', slug: 'media/cast-receiver' },
					],
				},
				{
					label: 'Screens & Workspace',
					items: [
						{ label: 'Phone Screen Mirroring', slug: 'workspace/screen-mirroring' },
						{ label: 'Sidecar (Virtual Display & Tablet)', slug: 'workspace/sidecar' },
						{ label: 'Universal Control', slug: 'workspace/universal-control' },
						{ label: 'Continuity Sketch & Markup', slug: 'workspace/sketch-markup' },
					],
				},
				{
					label: 'Ecosystem & Productivity',
					items: [
						{ label: 'Calls & Telephony Relay', slug: 'ecosystem/telephony' },
						{ label: 'Shared Notifications Hub', slug: 'ecosystem/notifications' },
						{ label: 'Biometric PAM & Sudo Approval', slug: 'ecosystem/biometric-auth' },
						{ label: 'Proximity Unlock & Security', slug: 'ecosystem/security' },
						{ label: 'Handoff & Desktop Widgets', slug: 'ecosystem/handoff-widgets' },
					],
				},
			],
		}),
	],
});
