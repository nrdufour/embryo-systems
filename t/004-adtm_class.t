#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
	etap:plan(unknown),
	etap_can:loaded_ok(adtm_class, "Module 'adtm_class' loaded."),
	etap_can:has_attrib(adtm_class, behavior),
	etap_can:is_attrib(adtm_class, behavior, gen_server),
	etap_can:can_ok(adtm_class, create, 1),
	etap_can:can_ok(adtm_class, hibern, 1),
	etap_can:can_ok(adtm_class, awake, 1),
	etap_can:can_ok(adtm_class, destroy, 1),
	etap_can:can_ok(adtm_class, resur, 1),
	etap_can:can_ok(adtm_class, purge, 1),
	etap:end_tests(),
	ok.

