#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
	etap:plan(unknown),
	etap_can:loaded_ok(embryosys_adtm_class, "Module 'embryosys_adtm_class' loaded"),
	etap_can:loaded_ok(embryosys_adtm_attribute, "Module 'embryosys_adtm_attribute' loaded"),
	etap_can:loaded_ok(embryosys_adtm_link, "Module 'embryosys_adtm_link' loaded"),
	etap_can:loaded_ok(embryosys_adtm_object, "Module 'embryosys_adtm_object' loaded"),
	etap_can:loaded_ok(adtm_server, "Module 'adtm_server' loaded"),
	etap_can:loaded_ok(embryosys_storage_server, "Module 'embryosys_storage_server' loaded"),
	etap_can:loaded_ok(embryosys_util, "Module 'embryosys_util' loaded"),
	etap_can:loaded_ok(embryosys, "Module 'embryosys' loaded"),
	etap_can:loaded_ok(embryosys_sup, "Module 'embryosys_sup' loaded"),
	etap:end_tests(),
	ok.
