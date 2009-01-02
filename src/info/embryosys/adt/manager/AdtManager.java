/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.manager;

import info.embryosys.adt.storage.impl.DummyStorage;

import java.util.LinkedList;
import java.util.List;

/**
 * @author nrdufour
 * 
 */
public class AdtManager {

	private List<Workspace> workspaces;

	/**
	 * 
	 */
	public AdtManager() {
		this.workspaces = new LinkedList<Workspace>();
	}
	
	public void init() {
		// FIXME for now it's hardcoded .. I will find a way to configure that through a conf file
		
		Workspace dummy = new Workspace("default", new DummyStorage());
		workspaces.add(dummy);
	}
}
